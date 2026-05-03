//! `kiln schema` — prints a JSON Schema for `Kiln.toml` to stdout.
//!
//! Wire this into your editor via `taplo` or VS Code's Even Better TOML
//! extension for completion and inline validation.

use anyhow::Result;
use serde_json::{json, Value};

pub fn run() -> Result<()> {
    let schema = build_schema();
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}

fn build_schema() -> Value {
    let severity = json!({
        "type": "string",
        "enum": ["error", "warn", "off", "deny"]
    });

    let trace_format = json!({
        "oneOf": [
            { "type": "boolean", "enum": [false] },
            { "type": "string", "enum": ["vcd", "fst"] }
        ],
        "description": "Trace output format: false (off), \"vcd\", or \"fst\""
    });

    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "Kiln.toml",
        "description": "Manifest for a kiln SystemVerilog project",
        "type": "object",
        "required": ["package", "design"],
        "additionalProperties": false,
        "properties": {
            "package": {
                "type": "object",
                "required": ["name", "version"],
                "additionalProperties": false,
                "properties": {
                    "name": { "type": "string", "description": "Package name (valid SV identifier)" },
                    "version": { "type": "string", "description": "Semver version string" },
                    "authors": { "type": "array", "items": { "type": "string" } },
                    "description": { "type": "string" },
                    "license": { "type": "string" }
                }
            },
            "design": {
                "type": "object",
                "required": ["top"],
                "additionalProperties": false,
                "properties": {
                    "top": { "type": "string", "description": "Top module name" },
                    "sources": {
                        "type": "array",
                        "items": { "type": "string" },
                        "default": ["src/**/*.sv", "src/**/*.svh", "src/**/*.v"]
                    },
                    "timescale": { "type": "string", "description": "e.g. \"1ns/1ps\"" },
                    "language": {
                        "type": "string",
                        "enum": ["sv2005", "sv2009", "sv2012", "sv2017", "sv2023"]
                    },
                    "include_dirs": { "type": "array", "items": { "type": "string" } },
                    "defines": {
                        "type": "object",
                        "additionalProperties": { "type": "string" }
                    },
                    "libraries": { "type": "array", "items": { "type": "string" } },
                    "test_sources": { "type": "array", "items": { "type": "string" } }
                }
            },
            "dependencies": {
                "type": "object",
                "additionalProperties": true,
                "description": "Bender-compatible dependency entries"
            },
            "lint": {
                "type": "object",
                "description": "Lint severity overrides",
                "properties": {
                    "slang": {
                        "type": "object",
                        "description": "Slang-specific lint rules",
                        "additionalProperties": severity.clone()
                    },
                    "verilator": {
                        "type": "object",
                        "description": "Verilator-specific lint rules (WIDTHTRUNC-style names)",
                        "additionalProperties": severity.clone()
                    }
                },
                "additionalProperties": severity.clone()
            },
            "tool": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "slang": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "path": { "type": "string" },
                            "extra_args": { "type": "array", "items": { "type": "string" } }
                        }
                    },
                    "verilator": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "path": { "type": "string" },
                            "threads": { "type": "integer", "minimum": 1 },
                            "trace": trace_format.clone(),
                            "coverage": { "type": "boolean" },
                            "extra_args": { "type": "array", "items": { "type": "string" } }
                        }
                    },
                    "verible": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "path": { "type": "string" },
                            "extra_args": { "type": "array", "items": { "type": "string" } }
                        }
                    }
                }
            },
            "profile": {
                "type": "object",
                "description": "Named build profiles (dev, release, test, or custom)",
                "additionalProperties": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "lint": {
                            "type": "object",
                            "additionalProperties": severity.clone()
                        },
                        "tool": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "slang": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "properties": {
                                        "path": { "type": "string" },
                                        "extra_args": { "type": "array", "items": { "type": "string" } }
                                    }
                                },
                                "verilator": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "properties": {
                                        "path": { "type": "string" },
                                        "threads": { "type": "integer", "minimum": 1 },
                                        "trace": trace_format,
                                        "coverage": { "type": "boolean" },
                                        "extra_args": { "type": "array", "items": { "type": "string" } }
                                    }
                                },
                                "verible": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "properties": {
                                        "path": { "type": "string" },
                                        "extra_args": { "type": "array", "items": { "type": "string" } }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "wave": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "format": { "type": "string", "enum": ["fst", "vcd"] },
                    "enabled_by_default": { "type": "boolean" }
                }
            }
        }
    })
}
