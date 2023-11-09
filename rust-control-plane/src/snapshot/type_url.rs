macro_rules! prefix {
    ($type:literal) => {
        concat!("type.googleapis.com/", $type)
    };
}

pub const ENDPOINT: &str = prefix!("envoy.config.endpoint.v3.ClusterLoadAssignment");
pub const CLUSTER: &str = prefix!("envoy.config.cluster.v3.Cluster");
pub const ROUTE: &str = prefix!("envoy.config.route.v3.RouteConfiguration");
pub const VIRTUAL_HOST: &str = prefix!("envoy.config.route.v3.VirtualHost");
pub const LISTENER: &str = prefix!("envoy.config.listener.v3.Listener");
pub const SECRET: &str = prefix!("envoy.extensions.transport_sockets.tls.v3.Secret");
pub const RUNTIME: &str = prefix!("envoy.service.runtime.v3.Runtime");
pub const SCOPED_ROUTE: &str = prefix!("envoy.config.route.v3.ScopedRouteConfiguration");
pub const EXTENSION_CONFIG: &str = prefix!("envoy.config.core.v3.TypedExtensionConfig");

pub const ANY_TYPE: &str = "";

pub fn shorten(type_url: &str) -> &str {
    if type_url.is_empty() {
        return "ADS";
    }
    type_url.split('.').last().unwrap_or(type_url)
}

pub fn priority(type_url: &str) -> u8 {
    match type_url {
        CLUSTER => 0,
        ENDPOINT => 1,
        LISTENER => 2,
        ROUTE => 3,
        VIRTUAL_HOST => 4,
        SECRET => 5,
        RUNTIME => 6,
        SCOPED_ROUTE => 7,
        EXTENSION_CONFIG => 8,
        _ => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_concatinates_valid_type() {
        assert_eq!(
            CLUSTER,
            "type.googleapis.com/envoy.config.cluster.v3.Cluster"
        )
    }
}
