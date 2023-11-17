#[allow(dead_code)]
pub(crate) fn build(autoheal_container_label: &str) -> serde_json::Value {
    let mut json = serde_json::Map::<String, serde_json::Value>::from_iter([(
        "health".into(),
        vec!["unhealthy"].into(),
    )]);

    if "all" != autoheal_container_label {
        let label_filter = format!("{}=true", autoheal_container_label);
        json.insert("label".into(), vec![label_filter].into());
    };

    serde_json::Value::Object(json)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::filters::build;

    #[test]
    fn test_build_filters_all() {
        let all_unhealthy = build("all");

        assert_eq!(all_unhealthy, json!({ "health": ["unhealthy"] }));
    }

    #[test]
    fn test_build_filters_autoheal() {
        let autoheal_and_unhealthy = build("autoheal");

        assert_eq!(
            autoheal_and_unhealthy,
            json!({ "health": ["unhealthy"], "label": ["autoheal=true"] })
        );
    }

    #[test]
    fn test_build_filters_custom() {
        let custom_and_unhealthy = build("custom");

        assert_eq!(
            custom_and_unhealthy,
            json!({ "health": ["unhealthy"], "label": ["custom=true"] })
        );
    }
}
