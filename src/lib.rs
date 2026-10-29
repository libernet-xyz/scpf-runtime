// Copyright 2026 The Libernet Team
// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]

pub mod starkom {
    pub mod scpf {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/starkom.scpf.v1.rs"));
        }
    }
}

pub mod varint;
