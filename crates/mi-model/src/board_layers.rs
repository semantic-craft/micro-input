//! Local board intent. This does not describe an installed device configuration.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct BoardLayer(u8);

impl BoardLayer {
    pub const ALL: [Self; 6] = [Self(1), Self(2), Self(3), Self(4), Self(5), Self(6)];

    pub const fn number(self) -> u8 {
        self.0
    }

    pub const fn offset(self) -> usize {
        self.0 as usize - 1
    }

    pub const fn leds(self) -> [bool; 3] {
        const PATTERNS: [[bool; 3]; 6] = [
            [true, false, false],
            [false, true, false],
            [false, false, true],
            [true, true, false],
            [false, true, true],
            [true, true, true],
        ];
        PATTERNS[self.offset()]
    }
}

impl TryFrom<u8> for BoardLayer {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if (1..=6).contains(&value) {
            Ok(Self(value))
        } else {
            Err("板载层编号必须为 1 至 6")
        }
    }
}

impl From<BoardLayer> for u8 {
    fn from(layer: BoardLayer) -> Self {
        layer.number()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NativeLayerOwner {
    Codex,
    Cindy,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum BoardLayerRoute {
    #[default]
    Unassigned,
    HostProfile {
        profile_id: String,
    },
    Native {
        owner: NativeLayerOwner,
        profile_id: String,
    },
}

impl BoardLayerRoute {
    pub fn profile_id(&self) -> Option<&str> {
        match self {
            Self::Unassigned => None,
            Self::HostProfile { profile_id } | Self::Native { profile_id, .. } => Some(profile_id),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardLayerPlan {
    layers: [BoardLayerRoute; 6],
}

impl BoardLayerPlan {
    pub fn recommended(
        native_codex: String,
        antigravity: String,
        deepseek: String,
        managed_cindy: String,
    ) -> Self {
        Self {
            layers: [
                BoardLayerRoute::Native {
                    owner: NativeLayerOwner::Codex,
                    profile_id: native_codex,
                },
                BoardLayerRoute::HostProfile {
                    profile_id: antigravity,
                },
                BoardLayerRoute::HostProfile {
                    profile_id: deepseek,
                },
                BoardLayerRoute::HostProfile {
                    profile_id: managed_cindy,
                },
                BoardLayerRoute::Unassigned,
                BoardLayerRoute::Unassigned,
            ],
        }
    }

    pub fn route(&self, layer: BoardLayer) -> &BoardLayerRoute {
        &self.layers[layer.offset()]
    }

    pub fn replace(&mut self, layer: BoardLayer, route: BoardLayerRoute) {
        self.layers[layer.offset()] = route;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum BoardPreviewSelection {
    #[default]
    UsbStartup,
    Layer(BoardLayer),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyThreeBindings(pub [Option<String>; 3]);

pub fn native_owner(
    document: &crate::document::LocalDocumentV2,
    profile_id: &str,
) -> Option<NativeLayerOwner> {
    let profile = document.editable.graph.profiles.get(profile_id)?;
    if !profile
        .layer_order
        .iter()
        .any(|id| document.is_protected_oai_layer(id))
    {
        return None;
    }
    match profile.name.as_str() {
        "Codex" => Some(NativeLayerOwner::Codex),
        "Cindy" => Some(NativeLayerOwner::Cindy),
        _ => None,
    }
}
