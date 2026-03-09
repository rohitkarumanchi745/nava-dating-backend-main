//! Client adaptive middleware: battery, temperature, and network-class aware
//! throttling for heavy operations like media uploads.

use serde::Serialize;

/// Network connection class reported by client.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum NetworkClass {
    Wifi,
    FiveG,
    FourG,
    ThreeG,
    TwoG,
    Offline,
    Unknown,
}

impl NetworkClass {
    pub fn from_header(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "wifi" => Self::Wifi,
            "5g" => Self::FiveG,
            "4g" | "lte" => Self::FourG,
            "3g" => Self::ThreeG,
            "2g" | "edge" | "gprs" => Self::TwoG,
            "offline" | "none" => Self::Offline,
            _ => Self::Unknown,
        }
    }

    /// Whether this network supports heavy uploads.
    pub fn supports_heavy_upload(&self) -> bool {
        matches!(self, Self::Wifi | Self::FiveG | Self::FourG)
    }

    /// Maximum recommended image size for this network class.
    pub fn max_image_size(&self) -> &'static str {
        match self {
            Self::TwoG | Self::ThreeG => "thumbnail",
            Self::FourG => "card",
            _ => "full",
        }
    }
}

/// Client device context extracted from request headers.
#[derive(Debug, Clone, Serialize)]
pub struct ClientContext {
    /// Battery level (0-100), None if not reported.
    pub battery_level: Option<u8>,
    /// Whether the device is charging.
    pub is_charging: Option<bool>,
    /// Network connection class.
    pub network_class: NetworkClass,
    /// Device temperature in Celsius, None if not reported.
    pub device_temp_celsius: Option<f32>,
}

/// Reason for deferring an upload.
#[derive(Debug, Clone, Serialize)]
pub struct DeferReason {
    pub reason: String,
    pub retry_after_secs: u64,
}

impl ClientContext {
    /// Extract client context from HTTP headers.
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let battery_level = headers.get("X-Battery-Level")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u8>().ok())
            .map(|v| v.min(100));

        let is_charging = headers.get("X-Is-Charging")
            .and_then(|v| v.to_str().ok())
            .map(|v| matches!(v, "1" | "true" | "yes"));

        let network_class = headers.get("X-Network-Class")
            .and_then(|v| v.to_str().ok())
            .map(|v| NetworkClass::from_header(v))
            .unwrap_or(NetworkClass::Unknown);

        let device_temp_celsius = headers.get("X-Device-Temp")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f32>().ok());

        Self {
            battery_level,
            is_charging,
            network_class,
            device_temp_celsius,
        }
    }

    /// Check if a heavy upload should be deferred.
    pub fn should_defer_upload(&self) -> Option<DeferReason> {
        // Low battery and not charging
        if let Some(battery) = self.battery_level {
            if battery < 15 && !self.is_charging.unwrap_or(false) {
                return Some(DeferReason {
                    reason: "low_battery".to_string(),
                    retry_after_secs: 300, // 5 minutes
                });
            }
        }

        // Device overheating
        if let Some(temp) = self.device_temp_celsius {
            if temp > 42.0 {
                return Some(DeferReason {
                    reason: "device_overheating".to_string(),
                    retry_after_secs: 180, // 3 minutes
                });
            }
        }

        // Poor network
        if matches!(self.network_class, NetworkClass::TwoG | NetworkClass::Offline) {
            return Some(DeferReason {
                reason: "poor_network".to_string(),
                retry_after_secs: 60,
            });
        }

        None
    }

    /// Get the response payload quality level for this client.
    pub fn payload_quality(&self) -> PayloadQuality {
        match &self.network_class {
            NetworkClass::TwoG | NetworkClass::ThreeG => PayloadQuality::Minimal,
            NetworkClass::FourG => PayloadQuality::Standard,
            _ => PayloadQuality::Full,
        }
    }
}

/// Quality level for response payloads.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PayloadQuality {
    /// 2G/3G: compressed JSON, thumbnail photos, no autoplay
    Minimal,
    /// 4G: standard payloads, card-size photos
    Standard,
    /// WiFi/5G: full resolution, HD content, prefetch
    Full,
}

impl PayloadQuality {
    /// Photo size suffix for this quality level.
    pub fn photo_size(&self) -> &'static str {
        match self {
            Self::Minimal => "thumbnail",
            Self::Standard => "card",
            Self::Full => "full",
        }
    }

    /// Whether to include reel previews in responses.
    pub fn include_reel_previews(&self) -> bool {
        !matches!(self, Self::Minimal)
    }

    /// Maximum number of items to return in list responses.
    pub fn page_size(&self) -> i32 {
        match self {
            Self::Minimal => 10,
            Self::Standard => 20,
            Self::Full => 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_class_parsing() {
        assert_eq!(NetworkClass::from_header("wifi"), NetworkClass::Wifi);
        assert_eq!(NetworkClass::from_header("4g"), NetworkClass::FourG);
        assert_eq!(NetworkClass::from_header("LTE"), NetworkClass::FourG);
        assert_eq!(NetworkClass::from_header("2g"), NetworkClass::TwoG);
        assert_eq!(NetworkClass::from_header("edge"), NetworkClass::TwoG);
    }

    #[test]
    fn test_defer_low_battery() {
        let ctx = ClientContext {
            battery_level: Some(10),
            is_charging: Some(false),
            network_class: NetworkClass::Wifi,
            device_temp_celsius: None,
        };
        assert!(ctx.should_defer_upload().is_some());
    }

    #[test]
    fn test_no_defer_charging() {
        let ctx = ClientContext {
            battery_level: Some(10),
            is_charging: Some(true),
            network_class: NetworkClass::Wifi,
            device_temp_celsius: None,
        };
        assert!(ctx.should_defer_upload().is_none());
    }

    #[test]
    fn test_defer_overheating() {
        let ctx = ClientContext {
            battery_level: Some(80),
            is_charging: None,
            network_class: NetworkClass::Wifi,
            device_temp_celsius: Some(45.0),
        };
        assert!(ctx.should_defer_upload().is_some());
    }

    #[test]
    fn test_defer_2g() {
        let ctx = ClientContext {
            battery_level: Some(80),
            is_charging: None,
            network_class: NetworkClass::TwoG,
            device_temp_celsius: None,
        };
        assert!(ctx.should_defer_upload().is_some());
    }

    #[test]
    fn test_payload_quality() {
        let ctx_2g = ClientContext {
            battery_level: None, is_charging: None,
            network_class: NetworkClass::TwoG,
            device_temp_celsius: None,
        };
        assert_eq!(ctx_2g.payload_quality(), PayloadQuality::Minimal);

        let ctx_wifi = ClientContext {
            battery_level: None, is_charging: None,
            network_class: NetworkClass::Wifi,
            device_temp_celsius: None,
        };
        assert_eq!(ctx_wifi.payload_quality(), PayloadQuality::Full);
    }
}
