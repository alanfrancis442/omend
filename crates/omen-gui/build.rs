// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compile the omend dashboard");
}
