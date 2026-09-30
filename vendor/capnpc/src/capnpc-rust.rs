// Copyright (c) 2013-2014 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! # Cap'n Proto Schema Compiler Plugin Executable
//!
//! [See this.](https://capnproto.org/otherlang.html#how-to-write-compiler-plugins)
//!
//!

pub fn main() {
    //! Generates Rust code according to a `schema_capnp::code_generator_request` read from stdin.

    let mut field_api = false;
    let mut values = false;
    let mut projections = false;
    for argument in ::std::env::args().skip(1) {
        match argument.as_str() {
            "--field-api" => field_api = true,
            "--field-api-values" => values = true,
            "--field-api-projections" => projections = true,
            "--help" | "-h" => {
                println!("capnpc-rust [--field-api] [--field-api-values] [--field-api-projections]\nReads a CodeGeneratorRequest from stdin and writes Rust bindings.\n--field-api adds the reader/field-operation API alongside existing bindings.\n--field-api-values adds allocating native values.\n--field-api-projections adds borrowed projections.");
                return;
            }
            _ => {
                eprintln!("unknown argument: {argument}");
                ::std::process::exit(2);
            }
        }
    }
    ::capnpc::codegen::CodeGenerationCommand::new()
        .field_api(field_api)
        .field_api_values(values)
        .field_api_projections(projections)
        .output_directory(::std::path::Path::new("."))
        .run(::std::io::stdin())
        .expect("failed to generate code");
}
