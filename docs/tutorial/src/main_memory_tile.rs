/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
#[allow(dead_code)]
fn main() {
    MainWindow::new().run();
}
sixtyfps::sixtyfps! {
// ANCHOR: tile
MemoryTile := Rectangle {
    width: 64px;
    height: 64px;
    background: #3960D5;

    Image {
        source: @image-url("icons/bus.png");
        width: parent.width;
        height: parent.height;
    }
}

MainWindow := Window {
    MemoryTile {}
}
// ANCHOR_END: tile
}
