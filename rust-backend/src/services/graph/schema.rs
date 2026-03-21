// =============================================================================
// Graph Schema — Property Graph model with typed nodes, edges, and directions
//
// Mirrors Netflix's Graph Schema layer: edge mappings define all valid
// relationships, enabling query planning and write validation.
// =============================================================================

use serde::{Deserialize, Serialize};
use std::fmt;

/// All node types in the Nava graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    User,
    University,
    Reel,
    Device,
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeType::User       => write!(f, "user"),
            NodeType::University => write!(f, "university"),
            NodeType::Reel       => write!(f, "reel"),
            NodeType::Device     => write!(f, "device"),
        }
    }
}

impl NodeType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "university" => NodeType::University,
            "reel"       => NodeType::Reel,
            "device"     => NodeType::Device,
            _            => NodeType::User,
        }
    }
}

/// All edge types in the Nava graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    MatchedWith,  // User <-> User (bidirectional)
    Liked,        // User  -> User
    Passed,       // User  -> User
    Attends,      // User  -> University
    Viewed,       // User  -> Reel
    Created,      // User  -> Reel
    Uses,         // User  -> Device
    Messaged,     // User  -> User
}

impl fmt::Display for EdgeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EdgeType::MatchedWith => write!(f, "matched_with"),
            EdgeType::Liked       => write!(f, "liked"),
            EdgeType::Passed      => write!(f, "passed"),
            EdgeType::Attends     => write!(f, "attends"),
            EdgeType::Viewed      => write!(f, "viewed"),
            EdgeType::Created     => write!(f, "created"),
            EdgeType::Uses        => write!(f, "uses"),
            EdgeType::Messaged    => write!(f, "messaged"),
        }
    }
}

impl EdgeType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "matched_with" => EdgeType::MatchedWith,
            "passed"       => EdgeType::Passed,
            "attends"      => EdgeType::Attends,
            "viewed"       => EdgeType::Viewed,
            "created"      => EdgeType::Created,
            "uses"         => EdgeType::Uses,
            "messaged"     => EdgeType::Messaged,
            _              => EdgeType::Liked,
        }
    }
}

/// Direction for edge traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Out,   // follow forward index (from → to)
    In,    // follow reverse index (to ← from)
    Both,  // union of Out and In (deduplicated)
}

/// A single valid edge relationship in the schema
#[derive(Debug, Clone)]
pub struct EdgeMapping {
    pub from_type:     NodeType,
    pub edge_type:     EdgeType,
    pub to_type:       NodeType,
    pub bidirectional: bool,
}

/// Nava graph schema — loaded on startup, validates writes and guides traversal planning
pub struct GraphSchema {
    pub mappings: Vec<EdgeMapping>,
}

impl GraphSchema {
    /// Build the canonical Nava schema
    pub fn nava() -> Self {
        Self {
            mappings: vec![
                // Social
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::MatchedWith, to_type: NodeType::User,       bidirectional: true  },
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Liked,       to_type: NodeType::User,       bidirectional: false },
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Passed,      to_type: NodeType::User,       bidirectional: false },
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Messaged,    to_type: NodeType::User,       bidirectional: false },
                // Education
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Attends,     to_type: NodeType::University, bidirectional: false },
                // Content
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Viewed,      to_type: NodeType::Reel,       bidirectional: false },
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Created,     to_type: NodeType::Reel,       bidirectional: false },
                // Device (fraud)
                EdgeMapping { from_type: NodeType::User, edge_type: EdgeType::Uses,        to_type: NodeType::Device,     bidirectional: false },
            ],
        }
    }

    /// Returns true if (from, edge, to) is a valid combination
    pub fn validate_edge(&self, from: NodeType, edge: EdgeType, to: NodeType) -> bool {
        self.mappings.iter().any(|m| {
            m.edge_type == edge
                && ((m.from_type == from && m.to_type == to)
                    || (m.bidirectional && m.from_type == to && m.to_type == from))
        })
    }

    /// Returns valid target node types for (from, edge)
    pub fn allowed_targets(&self, from: NodeType, edge: EdgeType) -> Vec<NodeType> {
        self.mappings
            .iter()
            .filter(|m| m.edge_type == edge && m.from_type == from)
            .map(|m| m.to_type)
            .collect()
    }

    pub fn is_bidirectional(&self, edge: EdgeType) -> bool {
        self.mappings.iter().any(|m| m.edge_type == edge && m.bidirectional)
    }
}
