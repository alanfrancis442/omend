// SPDX-License-Identifier: GPL-2.0-only
// SPDX-FileCopyrightText: 2026 alanfrancis442
/*
 * Keyboard zone backlight for HP Omen and Victus laptops.
 *
 * The BIOS exposes per-zone RGB through WMI method 0x20009 on the HP BIOS
 * GUID. hp-wmi on current kernels does not register LED class devices for
 * that method, so this module does, without taking the hotkey notify handler
 * hp-wmi already owns.
 *
 * Four-zone keyboards (type 0x01 with a numpad, 0x02 without) show up as
 * hp::kbd_zoned_backlight-{wasd,left,center,right}. Single-zone keyboards
 * (0x04, 0x05) show up as hp::kbd_backlight. omend writes those names.
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/acpi.h>
#include <linux/dmi.h>
#include <linux/led-class-multicolor.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/platform_device.h>
#include <linux/pm.h>
#include <linux/slab.h>
#include <linux/sysfs.h>
#include <linux/version.h>
#include <linux/workqueue.h>

#define OMEN_KBD_BIOS_GUID "5FB7F034-2C63-45E9-BE91-3D44E2C707E4"

#define OMEN_KBD_SIGNATURE 0x55434553

#define OMEN_KBD_READ 0x01
#define OMEN_KBD_GM 0x20008
#define OMEN_KBD_BACKLIGHT 0x20009

#define OMEN_KBD_HARDWARE_QUERY 0x04
#define OMEN_KBD_GET_KEYBOARD_TYPE 0x2b

#define OMEN_KBD_COLOR_GET 0x02
#define OMEN_KBD_COLOR_SET 0x03
#define OMEN_KBD_BRIGHTNESS_GET 0x04
#define OMEN_KBD_BRIGHTNESS_SET 0x05
#define OMEN_KBD_BACKLIGHT_OFF 0x64
#define OMEN_KBD_BACKLIGHT_ON 0xe4

#define OMEN_KBD_RET_INVALID_PARAMETERS 0x05

#define OMEN_KBD_COLOR_TABLE 128
#define OMEN_KBD_COLOR_OFFSET 25
#define OMEN_KBD_COLORS 3
#define OMEN_KBD_ZONE_MAX 4

enum omen_kbd_type {
	OMEN_KBD_NONE = 0x0,
	OMEN_KBD_FOURZONE_NUMPAD = 0x1,
	OMEN_KBD_FOURZONE = 0x2,
	OMEN_KBD_PER_KEY = 0x3,
	OMEN_KBD_SINGLE_NUMPAD = 0x4,
	OMEN_KBD_SINGLE = 0x5,
};

struct omen_bios_args {
	u32 signature;
	u32 command;
	u32 commandtype;
	u32 datasize;
	u8 data[];
};

struct omen_bios_return {
	u32 sigpass;
	u32 return_code;
};

struct omen_led {
	struct omen_kbd *kbd;
	int index;
	struct led_classdev_mc mc;
	struct mc_subled subleds[OMEN_KBD_COLORS];
};

struct omen_kbd {
	struct device *dev;
	struct mutex lock;
	struct delayed_work flush;
	bool zero_insize;
	bool dead;
	/* True after this driver last turned the backlight on. */
	bool lit;
	/* The backlight key turned the lamp off. Keep colors, but do not power it back on. */
	bool held_off;
	int num_zones;
	u8 rgb[OMEN_KBD_ZONE_MAX][OMEN_KBD_COLORS];
	struct omen_led leds[OMEN_KBD_ZONE_MAX];
};

static const char *const omen_kbd_zone_names[OMEN_KBD_ZONE_MAX] = {
	"hp::kbd_zoned_backlight-right",
	"hp::kbd_zoned_backlight-center",
	"hp::kbd_zoned_backlight-left",
	"hp::kbd_zoned_backlight-wasd",
};

static struct platform_device *omen_kbd_pdev;

static bool force;
module_param(force, bool, 0444);
MODULE_PARM_DESC(force, "Load on machines whose DMI data is not an HP OMEN or Victus (default: false)");

/*
 * The BIOS GUID is present on most HP laptops. Only issue the gaming keyboard
 * method on the product lines it is meant for unless force=1 is given.
 */
static const struct dmi_system_id omen_kbd_dmi_table[] __initconst = {
	{
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "HP"),
			DMI_MATCH(DMI_PRODUCT_NAME, "OMEN"),
		},
	},
	{
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "HP"),
			DMI_MATCH(DMI_PRODUCT_NAME, "Victus"),
		},
	},
	{ }
};

static int omen_kbd_method(int outsize)
{
	if (outsize > 4096)
		return -EINVAL;
	if (outsize > 1024)
		return 5;
	if (outsize > 128)
		return 4;
	if (outsize > 4)
		return 3;
	if (outsize > 0)
		return 2;
	return 1;
}

static int omen_kbd_wmi(int query, u32 command, void *buffer, int insize, int outsize)
{
	struct acpi_buffer input, output = { ACPI_ALLOCATE_BUFFER, NULL };
	struct omen_bios_return *bios_return;
	struct omen_bios_args *args;
	union acpi_object *obj;
	int actual_insize, actual_outsize, mid, ret;
	size_t args_size;
	acpi_status status;

	mid = omen_kbd_method(outsize);
	if (mid < 0)
		return mid;

	actual_insize = max(insize, OMEN_KBD_COLOR_TABLE);
	args_size = struct_size(args, data, actual_insize);
	args = kzalloc(args_size, GFP_KERNEL);
	if (!args)
		return -ENOMEM;

	args->signature = OMEN_KBD_SIGNATURE;
	args->command = command;
	args->commandtype = query;
	args->datasize = insize;
	if (insize > 0)
		memcpy(args->data, buffer, insize);

	input.length = args_size;
	input.pointer = args;

	status = wmi_evaluate_method(OMEN_KBD_BIOS_GUID, 0, mid, &input, &output);
	kfree(args);
	if (ACPI_FAILURE(status))
		return -EIO;

	obj = output.pointer;
	if (!obj || obj->type != ACPI_TYPE_BUFFER ||
	    obj->buffer.length < sizeof(*bios_return) || !obj->buffer.pointer) {
		kfree(obj);
		return -EINVAL;
	}

	bios_return = (struct omen_bios_return *)obj->buffer.pointer;
	/*
	 * Firmware status codes are positive. Do not let a large u32 turn into a
	 * negative value that callers would pass to userspace as an errno.
	 */
	ret = bios_return->return_code > INT_MAX ? INT_MAX : bios_return->return_code;
	if (!ret && outsize) {
		actual_outsize = min_t(int, outsize,
				       obj->buffer.length - sizeof(*bios_return));
		/* Never fabricate missing bytes in a read-modify-write color table. */
		if (command == OMEN_KBD_BACKLIGHT && query == OMEN_KBD_COLOR_GET &&
		    actual_outsize < outsize) {
			kfree(obj);
			return -EMSGSIZE;
		}
		memcpy(buffer, obj->buffer.pointer + sizeof(*bios_return), actual_outsize);
		if (actual_outsize < outsize)
			memset(buffer + actual_outsize, 0, outsize - actual_outsize);
	}

	kfree(obj);
	return ret;
}

static int omen_kbd_wmi_ok(int query, u32 command, void *buffer, int insize, int outsize)
{
	int ret = omen_kbd_wmi(query, command, buffer, insize, outsize);

	if (ret > 0)
		return -EIO;
	return ret;
}

static int omen_kbd_color_get(struct omen_kbd *kbd, u8 table[OMEN_KBD_COLOR_TABLE])
{
	int insize = kbd->zero_insize ? 0 : OMEN_KBD_COLOR_TABLE;

	memset(table, 0, OMEN_KBD_COLOR_TABLE);
	return omen_kbd_wmi_ok(OMEN_KBD_COLOR_GET, OMEN_KBD_BACKLIGHT, table, insize,
			       OMEN_KBD_COLOR_TABLE);
}

static int omen_kbd_set_backlight(struct omen_kbd *kbd, bool on);
static int omen_kbd_backlight_on(struct omen_kbd *kbd, bool *on);

static int omen_kbd_color_commit(struct omen_kbd *kbd, const u8 rgb[][OMEN_KBD_COLORS])
{
	u8 table[OMEN_KBD_COLOR_TABLE];
	int i, ret;

	ret = omen_kbd_color_get(kbd, table);
	if (ret)
		return ret;

	for (i = 0; i < kbd->num_zones; i++) {
		int off = OMEN_KBD_COLOR_OFFSET + i * OMEN_KBD_COLORS;

		table[off] = rgb[i][0];
		table[off + 1] = rgb[i][1];
		table[off + 2] = rgb[i][2];
	}
	return omen_kbd_wmi_ok(OMEN_KBD_COLOR_SET, OMEN_KBD_BACKLIGHT, table,
			       OMEN_KBD_COLOR_TABLE, OMEN_KBD_COLOR_TABLE);
}

static void omen_kbd_flush(struct work_struct *work)
{
	struct omen_kbd *kbd = container_of(to_delayed_work(work), struct omen_kbd, flush);
	u8 rgb[OMEN_KBD_ZONE_MAX][OMEN_KBD_COLORS];
	bool any = false;
	int i, ret;

	mutex_lock(&kbd->lock);
	if (kbd->dead) {
		mutex_unlock(&kbd->lock);
		return;
	}
	memcpy(rgb, kbd->rgb, sizeof(rgb));

	for (i = 0; i < kbd->num_zones; i++) {
		if (rgb[i][0] || rgb[i][1] || rgb[i][2])
			any = true;
	}

	/*
	 * A zero write is an explicit off from userspace, so it clears the key latch.
	 * Otherwise, if the backlight key turned the lamp off since we last lit it,
	 * leave it off until that key turns it back on.
	 */
	if (!any) {
		kbd->held_off = false;
		kbd->lit = false;
		omen_kbd_set_backlight(kbd, false);
		goto out;
	}

	if (kbd->held_off) {
		bool on = false;

		if (omen_kbd_backlight_on(kbd, &on) || !on)
			goto out;
		kbd->held_off = false;
	} else if (kbd->lit) {
		bool on = true;

		if (!omen_kbd_backlight_on(kbd, &on) && !on) {
			kbd->held_off = true;
			kbd->lit = false;
			goto out;
		}
	}

	ret = omen_kbd_set_backlight(kbd, true);
	if (ret) {
		dev_err_ratelimited(kbd->dev, "keyboard backlight on failed: %d\n", ret);
		goto out;
	}
	kbd->lit = true;
	ret = omen_kbd_color_commit(kbd, rgb);
	if (ret)
		dev_err_ratelimited(kbd->dev, "keyboard color update failed: %d\n", ret);
out:
	mutex_unlock(&kbd->lock);
}

static ssize_t backlight_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct omen_kbd *kbd = dev_get_drvdata(dev);
	bool on = true;
	int ret;

	if (!kbd)
		return -ENODEV;

	mutex_lock(&kbd->lock);
	if (kbd->dead) {
		mutex_unlock(&kbd->lock);
		return -ENODEV;
	}
	ret = omen_kbd_backlight_on(kbd, &on);
	mutex_unlock(&kbd->lock);
	if (ret)
		return ret;
	return sysfs_emit(buf, "%d\n", on ? 1 : 0);
}
/*
 * Every read evaluates a firmware method. Only omend (root) needs this state,
 * so do not let unprivileged users trigger ACPI calls at will.
 */
static DEVICE_ATTR_ADMIN_RO(backlight);

static struct attribute *omen_kbd_attrs[] = {
	&dev_attr_backlight.attr,
	NULL,
};
ATTRIBUTE_GROUPS(omen_kbd);

static int omen_kbd_backlight_on(struct omen_kbd *kbd, bool *on)
{
	u8 data = 0;
	int ret;

	ret = omen_kbd_wmi_ok(OMEN_KBD_BRIGHTNESS_GET, OMEN_KBD_BACKLIGHT, &data,
			      kbd->zero_insize ? 0 : sizeof(data), sizeof(data));
	if (ret)
		return ret;

	*on = data == OMEN_KBD_BACKLIGHT_ON;
	return 0;
}

static int omen_kbd_set_backlight(struct omen_kbd *kbd, bool on)
{
	u8 data = on ? OMEN_KBD_BACKLIGHT_ON : OMEN_KBD_BACKLIGHT_OFF;

	return omen_kbd_wmi_ok(OMEN_KBD_BRIGHTNESS_SET, OMEN_KBD_BACKLIGHT, &data,
			       sizeof(data), sizeof(data));
}

static int omen_kbd_brightness_set(struct led_classdev *led_cdev, enum led_brightness brightness)
{
	struct led_classdev_mc *mc = lcdev_to_mccdev(led_cdev);
	struct omen_led *led = container_of(mc, struct omen_led, mc);
	struct omen_kbd *kbd = led->kbd;

	mutex_lock(&kbd->lock);
	if (kbd->dead) {
		mutex_unlock(&kbd->lock);
		return 0;
	}
	led_cdev->brightness = brightness;
	led_mc_calc_color_components(mc, brightness);
	kbd->rgb[led->index][0] = mc->subled_info[0].brightness;
	kbd->rgb[led->index][1] = mc->subled_info[1].brightness;
	kbd->rgb[led->index][2] = mc->subled_info[2].brightness;
	mod_delayed_work(system_wq, &kbd->flush, msecs_to_jiffies(20));
	mutex_unlock(&kbd->lock);
	return 0;
}

static int omen_kbd_register_zone(struct omen_kbd *kbd, int zone, const u8 *table, bool on)
{
	struct omen_led *entry = &kbd->leds[zone];
	struct led_classdev_mc *mc = &entry->mc;
	struct led_classdev *led = &mc->led_cdev;
	struct mc_subled *sub = entry->subleds;
	const char *name;
	int i, ret;

	entry->kbd = kbd;
	entry->index = zone;

	if (kbd->num_zones == 1)
		name = "hp::kbd_backlight";
	else
		name = omen_kbd_zone_names[zone];

	led->name = devm_kstrdup(kbd->dev, name, GFP_KERNEL);
	if (!led->name)
		return -ENOMEM;

	led->max_brightness = LED_FULL;
	led->brightness = on ? LED_FULL : LED_OFF;
	led->brightness_set_blocking = omen_kbd_brightness_set;
	led->flags = LED_CORE_SUSPENDRESUME | LED_RETAIN_AT_SHUTDOWN;

	for (i = 0; i < OMEN_KBD_COLORS; i++) {
		sub[i].color_index = i == 0 ? LED_COLOR_ID_RED :
				     i == 1 ? LED_COLOR_ID_GREEN : LED_COLOR_ID_BLUE;
		sub[i].channel = zone * OMEN_KBD_COLORS + i;
		sub[i].intensity = table[OMEN_KBD_COLOR_OFFSET + sub[i].channel];
		sub[i].brightness = LED_FULL;
	}

	mc->subled_info = sub;
	mc->num_colors = OMEN_KBD_COLORS;
	ret = devm_led_classdev_multicolor_register(kbd->dev, mc);
	if (ret)
		dev_err(kbd->dev, "failed to register %s: %d\n", name, ret);
	return ret;
}

static void omen_kbd_cancel_work(void *data)
{
	struct omen_kbd *kbd = data;

	mutex_lock(&kbd->lock);
	kbd->dead = true;
	mutex_unlock(&kbd->lock);
	cancel_delayed_work_sync(&kbd->flush);
}

static int omen_kbd_probe(struct platform_device *pdev)
{
	struct omen_kbd *kbd;
	u8 keyboard_type = 0;
	u8 table[OMEN_KBD_COLOR_TABLE];
	u8 probe_buf[4] = {};
	bool on = false;
	int zones, i, ret;

	kbd = devm_kzalloc(&pdev->dev, sizeof(*kbd), GFP_KERNEL);
	if (!kbd)
		return -ENOMEM;

	kbd->dev = &pdev->dev;
	mutex_init(&kbd->lock);
	INIT_DELAYED_WORK(&kbd->flush, omen_kbd_flush);
	platform_set_drvdata(pdev, kbd);
	/*
	 * Also cancel work after a partially successful probe: remove() is not
	 * called then. LIFO devres ordering unregisters later LED devices first,
	 * cancels any work their callbacks queued, then frees kbd.
	 */
	ret = devm_add_action_or_reset(&pdev->dev, omen_kbd_cancel_work, kbd);
	if (ret)
		return ret;

	/*
	 * hp-wmi treats INVALID_PARAMETERS on the hardware query as "this BIOS
	 * wants a zero input size for reads".
	 */
	ret = omen_kbd_wmi(OMEN_KBD_HARDWARE_QUERY, OMEN_KBD_READ, probe_buf,
			   sizeof(probe_buf), sizeof(probe_buf));
	kbd->zero_insize = (ret == OMEN_KBD_RET_INVALID_PARAMETERS);

	ret = omen_kbd_wmi_ok(OMEN_KBD_GET_KEYBOARD_TYPE, OMEN_KBD_GM, &keyboard_type,
			   kbd->zero_insize ? 0 : sizeof(keyboard_type), sizeof(keyboard_type));
	if (ret) {
		dev_err(&pdev->dev, "keyboard type query failed: %d\n", ret);
		return ret;
	}

	switch (keyboard_type) {
	case OMEN_KBD_FOURZONE_NUMPAD:
	case OMEN_KBD_FOURZONE:
		zones = 4;
		break;
	case OMEN_KBD_SINGLE_NUMPAD:
	case OMEN_KBD_SINGLE:
		zones = 1;
		break;
	case OMEN_KBD_NONE:
		dev_info(&pdev->dev, "BIOS reports no keyboard backlight\n");
		return -ENODEV;
	case OMEN_KBD_PER_KEY:
		dev_info(&pdev->dev, "per-key keyboard is not a zone backlight\n");
		return -ENODEV;
	default:
		dev_info(&pdev->dev, "unsupported keyboard type 0x%x\n", keyboard_type);
		return -ENODEV;
	}

	ret = omen_kbd_color_get(kbd, table);
	if (ret) {
		dev_err(&pdev->dev, "failed to read keyboard colors: %d\n", ret);
		return ret;
	}

	ret = omen_kbd_backlight_on(kbd, &on);
	if (ret)
		on = true;

	kbd->num_zones = zones;
	for (i = 0; i < zones; i++) {
		int off = OMEN_KBD_COLOR_OFFSET + i * OMEN_KBD_COLORS;

		kbd->rgb[i][0] = table[off];
		kbd->rgb[i][1] = table[off + 1];
		kbd->rgb[i][2] = table[off + 2];
		ret = omen_kbd_register_zone(kbd, i, table, on);
		if (ret)
			return ret;
	}

	dev_info(&pdev->dev, "keyboard type 0x%x, %d zone(s)\n", keyboard_type, zones);
	return 0;
}

static void omen_kbd_remove(struct platform_device *pdev)
{
	struct omen_kbd *kbd = platform_get_drvdata(pdev);

	omen_kbd_cancel_work(kbd);
}

/*
 * LED_CORE_SUSPENDRESUME turns each zone off before this parent suspends. Run
 * the coalesced write now, while firmware is still available, instead of
 * letting the delayed work fire in the middle of the system transition.
 */
static int omen_kbd_suspend(struct device *dev)
{
	struct omen_kbd *kbd = dev_get_drvdata(dev);

	flush_delayed_work(&kbd->flush);
	return 0;
}

static DEFINE_SIMPLE_DEV_PM_OPS(omen_kbd_pm_ops, omen_kbd_suspend, NULL);

static struct platform_driver omen_kbd_driver = {
	.probe = omen_kbd_probe,
#if LINUX_VERSION_CODE < KERNEL_VERSION(6, 11, 0)
	.remove_new = omen_kbd_remove,
#else
	.remove = omen_kbd_remove,
#endif
	.driver = {
		.name = "omen-kbd",
		.dev_groups = omen_kbd_groups,
		.pm = pm_sleep_ptr(&omen_kbd_pm_ops),
	},
};

static int __init omen_kbd_init(void)
{
	int ret;

	if (!wmi_has_guid(OMEN_KBD_BIOS_GUID))
		return -ENODEV;

	if (!dmi_check_system(omen_kbd_dmi_table)) {
		if (!force) {
			pr_info("not an HP OMEN or Victus system; use force=1 to override\n");
			return -ENODEV;
		}
		pr_warn("loading on an unrecognized system because force=1\n");
	}

	ret = platform_driver_register(&omen_kbd_driver);
	if (ret)
		return ret;

	omen_kbd_pdev = platform_device_register_simple("omen-kbd", PLATFORM_DEVID_NONE, NULL, 0);
	if (IS_ERR(omen_kbd_pdev)) {
		ret = PTR_ERR(omen_kbd_pdev);
		platform_driver_unregister(&omen_kbd_driver);
		return ret;
	}

	/* Probe is synchronous. Do not stay loaded with nothing bound. */
	if (!omen_kbd_pdev->dev.driver) {
		platform_device_unregister(omen_kbd_pdev);
		platform_driver_unregister(&omen_kbd_driver);
		return -ENODEV;
	}

	return 0;
}

static void __exit omen_kbd_exit(void)
{
	platform_device_unregister(omen_kbd_pdev);
	platform_driver_unregister(&omen_kbd_driver);
}

module_init(omen_kbd_init);
module_exit(omen_kbd_exit);

MODULE_DESCRIPTION("HP Omen and Victus keyboard zone backlight");
MODULE_LICENSE("GPL");
MODULE_SOFTDEP("pre: hp-wmi");
