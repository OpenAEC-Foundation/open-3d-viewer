//! Compile-time product identity. Build each desktop product in a separate Cargo invocation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Product {
    pub id: &'static str,
    pub name: &'static str,
    pub settings_namespace: &'static str,
    pub bundle_id: &'static str,
    pub remote_env: &'static str,
    pub remote_port_env: &'static str,
    pub validation_enabled: bool,
}

pub const VIEWER: Product = Product {
    id: "open-3d-viewer",
    name: "Open 3D Viewer Studio",
    settings_namespace: "Open 3D Viewer Studio",
    bundle_id: "org.openaec.Open3DViewer",
    remote_env: "O3V_REMOTE",
    remote_port_env: "O3V_REMOTE_PORT",
    validation_enabled: false,
};

pub const VALIDATOR: Product = Product {
    id: "open-bim-validator",
    name: "Open BIM Validator",
    settings_namespace: "Open BIM Validator",
    bundle_id: "org.openaec.OpenBIMValidator",
    remote_env: "OBV_REMOTE",
    remote_port_env: "OBV_REMOTE_PORT",
    validation_enabled: true,
};

#[cfg(not(feature = "bim-validator"))]
pub const CURRENT: Product = VIEWER;
#[cfg(feature = "bim-validator")]
pub const CURRENT: Product = VALIDATOR;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_and_user_state_namespaces_are_independent() {
        assert_ne!(VIEWER.id, VALIDATOR.id);
        assert_ne!(VIEWER.name, VALIDATOR.name);
        assert_ne!(VIEWER.settings_namespace, VALIDATOR.settings_namespace);
        assert_ne!(VIEWER.bundle_id, VALIDATOR.bundle_id);
        assert_ne!(VIEWER.remote_env, VALIDATOR.remote_env);
        assert_ne!(VIEWER.remote_port_env, VALIDATOR.remote_port_env);
    }

    #[test]
    fn feature_selects_the_entire_product_identity() {
        assert_eq!(
            CURRENT,
            if cfg!(feature = "bim-validator") {
                VALIDATOR
            } else {
                VIEWER
            }
        );
        assert_eq!(CURRENT.validation_enabled, cfg!(feature = "bim-validator"));
    }
}
