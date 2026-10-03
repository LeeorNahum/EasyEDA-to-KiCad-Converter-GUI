//! EasyEDA to KiCad Converter: fetches a part from EasyEDA by its LCSC
//! number and writes a KiCad symbol, footprint, and 3D model.
//!
//! The conversion is a port of easyeda2kicad.py 1.0.1
//! (<https://github.com/uPesy/easyeda2kicad.py>, AGPL-3.0). `main.rs` opens
//! the window, or runs `cli` when it is given arguments. This library target
//! lets the tests in `tests/` drive the real conversion without a window.

pub mod cli;
pub mod commands;
pub mod convert;
pub mod easyeda;
pub mod kicad;
pub mod lock;
pub mod messages;
pub mod project;
pub mod pyfmt;
