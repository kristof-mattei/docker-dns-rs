#[allow(dead_code)]
pub(crate) fn url_encode(filter: &serde_json::Value) -> String {
    percent_encoding::percent_encode(
        (filter).to_string().as_bytes(),
        percent_encoding::NON_ALPHANUMERIC,
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use crate::encoding::url_encode;
    use crate::filters::build;

    #[test]
    fn test_build_decode_all() {
        let all_unhealthy = build("all");

        let all_unhealthy_encoded = url_encode(&all_unhealthy);

        assert_eq!(
            all_unhealthy_encoded,
            "%7B%22health%22%3A%5B%22unhealthy%22%5D%7D"
        );
    }

    #[test]
    fn test_build_decode_autoheal() {
        let autoheal_and_unhealthy = build("autoheal");

        let autoheal_and_unhealthy_encoded = url_encode(&autoheal_and_unhealthy);

        assert_eq!(autoheal_and_unhealthy_encoded, "%7B%22health%22%3A%5B%22unhealthy%22%5D%2C%22label%22%3A%5B%22autoheal%3Dtrue%22%5D%7D");
    }

    #[test]
    fn test_build_decode_custom() {
        let custom_and_unhealthy = build("custom");

        let custom_and_unhealthy_encoded = url_encode(&custom_and_unhealthy);

        assert_eq!(
            custom_and_unhealthy_encoded,
            "%7B%22health%22%3A%5B%22unhealthy%22%5D%2C%22label%22%3A%5B%22custom%3Dtrue%22%5D%7D"
        );
    }
}
