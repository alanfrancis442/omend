obj-m += omen-kbd.o

KDIR ?= /lib/modules/$(shell uname -r)/build
PWD := $(CURDIR)
# Refresh the module index of the target kernel, not the running one.
KREL = $(shell cat $(KDIR)/include/config/kernel.release)

all:
	$(MAKE) -C $(KDIR) M=$(PWD) modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean

install: all
	$(MAKE) -C $(KDIR) M=$(PWD) modules_install
	depmod -a "$(KREL)"

.PHONY: all clean install
