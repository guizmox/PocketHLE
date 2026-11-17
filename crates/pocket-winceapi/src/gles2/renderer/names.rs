//! Translate guest object identifiers when a restored share group allocates
//! different native names.
use std::collections::{hash_map::Entry, HashMap};

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(super) enum Kind {
    Buffer,
    Texture,
    Framebuffer,
    Renderbuffer,
    Program,
    Shader,
}

impl Kind {
    pub(super) fn generated(name: &str) -> Option<Self> {
        match name {
            "glGenBuffers" | "glDeleteBuffers" => Some(Self::Buffer),
            "glGenTextures" | "glDeleteTextures" => Some(Self::Texture),
            "glGenFramebuffers" | "glDeleteFramebuffers" => Some(Self::Framebuffer),
            "glGenRenderbuffers" | "glDeleteRenderbuffers" => Some(Self::Renderbuffer),
            _ => None,
        }
    }

    pub(super) fn argument(name: &str, index: u8) -> Option<Self> {
        match (name, index) {
            ("glBindBuffer", 1) | ("glIsBuffer", 0) => Some(Self::Buffer),
            ("glBindTexture", 1) | ("glIsTexture", 0) | ("glFramebufferTexture2D", 3) => {
                Some(Self::Texture)
            }
            ("glBindFramebuffer", 1) | ("glIsFramebuffer", 0) => Some(Self::Framebuffer),
            ("glBindRenderbuffer", 1)
            | ("glIsRenderbuffer", 0)
            | ("glFramebufferRenderbuffer", 3) => Some(Self::Renderbuffer),
            ("glAttachShader" | "glDetachShader", 1)
            | (
                "glIsShader" | "glCompileShader" | "glDeleteShader" | "glShaderSource"
                | "glGetShaderiv" | "glGetShaderInfoLog" | "glGetShaderSource",
                0,
            ) => Some(Self::Shader),
            (
                "glAttachShader"
                | "glDetachShader"
                | "glIsProgram"
                | "glLinkProgram"
                | "glValidateProgram"
                | "glUseProgram"
                | "glDeleteProgram"
                | "glGetProgramiv"
                | "glGetProgramInfoLog"
                | "glGetAttachedShaders"
                | "glGetActiveAttrib"
                | "glGetActiveUniform"
                | "glGetUniformfv"
                | "glGetUniformiv"
                | "glGetUniformLocation"
                | "glGetAttribLocation"
                | "glBindAttribLocation",
                0,
            ) => Some(Self::Program),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Namespace {
    guest_to_host: HashMap<u32, u32>,
    host_to_guest: HashMap<u32, u32>,
}

#[derive(Default)]
pub(super) struct ObjectNames {
    namespaces: [Namespace; 6],
    pub(super) remapped: bool,
}

impl ObjectNames {
    pub(super) fn pairs(&self, kind: Kind) -> Vec<(u32, u32)> {
        let mut pairs: Vec<_> = self.namespaces[kind as usize]
            .guest_to_host
            .iter()
            .map(|(&guest, &host)| (guest, host))
            .collect();
        pairs.sort_unstable();
        pairs
    }

    pub(super) fn assign(&mut self, kind: Kind, host: u32, saved: Option<u32>) -> Option<u32> {
        if host == 0 || saved == Some(0) {
            return (saved.is_none_or(|saved| saved == host)).then_some(0);
        }
        let names = &mut self.namespaces[kind as usize];
        // A deleted program or shader can survive until its last reference.
        // A new allocation of that native name is proof it has been released.
        if let Some(old_guest) = names.host_to_guest.remove(&host) {
            names.guest_to_host.remove(&old_guest);
        }
        let guest = match saved {
            Some(guest) => guest,
            None if !names.guest_to_host.contains_key(&host) => host,
            None => (1..u32::MAX).find(|name| !names.guest_to_host.contains_key(name))?,
        };
        match names.guest_to_host.entry(guest) {
            Entry::Occupied(_) => return None,
            Entry::Vacant(entry) => {
                entry.insert(host);
            }
        }
        names.host_to_guest.insert(host, guest);
        self.remapped |= host != guest;
        Some(guest)
    }

    pub(super) fn host(&self, kind: Kind, guest: u32) -> u32 {
        if !self.remapped {
            return guest;
        }
        let names = &self.namespaces[kind as usize];
        names
            .guest_to_host
            .get(&guest)
            .copied()
            // An unknown guest name cannot refer to an already-remapped object.
            .unwrap_or_else(|| {
                if names.host_to_guest.contains_key(&guest) {
                    u32::MAX
                } else {
                    guest
                }
            })
    }

    pub(super) fn guest(&self, kind: Kind, host: u32) -> u32 {
        self.namespaces[kind as usize]
            .host_to_guest
            .get(&host)
            .copied()
            .unwrap_or(host)
    }

    pub(super) fn remove(&mut self, kind: Kind, guest: u32) {
        let names = &mut self.namespaces[kind as usize];
        if let Some(host) = names.guest_to_host.remove(&guest) {
            names.host_to_guest.remove(&host);
        }
    }
}
