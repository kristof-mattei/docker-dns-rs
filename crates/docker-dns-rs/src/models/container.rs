use std::fmt;

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::models::container_inspect::ContainerNetworkSettings;

fn deserialize_names<'de, D>(deserializer: D) -> Result<Box<[Box<str>]>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqVisitor();

    impl<'de> Visitor<'de> for SeqVisitor {
        type Value = Box<[Box<str>]>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a nonempty sequence of items")
        }

        fn visit_seq<M>(self, mut seq: M) -> Result<Self::Value, M::Error>
        where
            M: SeqAccess<'de>,
        {
            let mut buffer = seq.size_hint().map_or_else(Vec::new, Vec::with_capacity);

            while let Some(mut value) = seq.next_element::<String>()? {
                // Docker container name starts with a '/'. I don't know why. But it's useless.
                if value.starts_with('/') {
                    let split = value.split_off(1);

                    buffer.push(split.into_boxed_str());
                } else {
                    buffer.push(value.into_boxed_str());
                }
            }

            Ok(buffer.into())
        }
    }

    let visitor = SeqVisitor();
    deserializer.deserialize_seq(visitor)
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Container {
    pub id: Box<str>,
    #[serde(deserialize_with = "deserialize_names")]
    #[serde(rename(deserialize = "Names"))]
    pub names: Box<[Box<str>]>,
    pub state: Box<str>,
    pub network_settings: ContainerNetworkSettings,
}

#[cfg(test)]
mod tests {
    use hashbrown::HashMap;
    use pretty_assertions::assert_eq;

    use crate::models::container::Container;
    use crate::models::container_inspect::ContainerNetworkSettings;

    fn to_tuple(container: &Container) -> (&str, &[Box<str>], &str) {
        let &Container {
            ref id,
            ref names,
            ref state,
            ..
        } = container;

        (id, names, state)
    }

    #[test]
    fn deserialize() {
        let input = r#"[{"Id":"582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae","Names":["/photoprism"],"NetworkSettings":{"Networks":{}},"Labels":{},"State":"running"},{"Id":"281ea0c72e2e4a41fd2f81df945da9dfbfbc7ea0fe5e59c3d2a8234552e367cf","Names":["/whoogle-search"],"NetworkSettings":{"Networks":{}},"Labels":{},"State":"running"}]"#;

        let deserialized: Result<Vec<Container>, _> = serde_json::from_slice(input.as_bytes());

        assert!(deserialized.is_ok());

        assert_eq!(
            &[
                to_tuple(&Container {
                    id: "582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae".into(),
                    names: ["photoprism".into()].into(),
                    network_settings: ContainerNetworkSettings {
                        networks: HashMap::default()
                    },
                    state: "running".into(),
                }),
                to_tuple(&Container {
                    id: "281ea0c72e2e4a41fd2f81df945da9dfbfbc7ea0fe5e59c3d2a8234552e367cf".into(),
                    names: ["whoogle-search".into()].into(),
                    network_settings: ContainerNetworkSettings {
                        networks: HashMap::default()
                    },
                    state: "running".into(),
                }),
            ][..],
            deserialized
                .unwrap()
                .iter()
                .map(to_tuple)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn deserialize_multiple_names() {
        let input = r#"[{"Id":"582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae","Names":["/photoprism-1","/photoprism-2"],"NetworkSettings":{"Networks":{}},"Labels":{},"State":"running"}]"#;

        let deserialized: Result<Vec<Container>, _> = serde_json::from_slice(input.as_bytes());

        assert!(deserialized.is_ok());

        assert_eq!(
            &[to_tuple(&Container {
                id: "582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae".into(),
                names: ["photoprism-1".into(), "photoprism-2".into()].into(),
                network_settings: ContainerNetworkSettings {
                    networks: HashMap::new(),
                },
                state: "running".into(),
            })][..],
            deserialized
                .unwrap()
                .iter()
                .map(to_tuple)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn deserialize_with_no_names_array() {
        let input = r#"[{"Id":"582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae","State":"running","NetworkSettings":{"Networks":{}},"Labels":{"autoheal.stop.other_label":"some_value"}}]"#;

        let deserialized: Result<Vec<Container>, _> = serde_json::from_slice(input.as_bytes());

        deserialized.unwrap_err();
    }

    #[test]
    fn deserialize_names_empty_names_array() {
        let input = r#"[{"Id":"582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae","Names":[],"NetworkSettings":{"Networks":{}},"State":"running","Labels":{"autoheal.stop.other_label":"some_value"}}]"#;

        let deserialized: Result<Vec<Container>, _> = serde_json::from_slice(input.as_bytes());

        assert!(deserialized.is_ok());

        assert_eq!(
            &[to_tuple(&Container {
                id: "582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae".into(),
                names: vec![].into(),
                network_settings: ContainerNetworkSettings {
                    networks: HashMap::new(),
                },
                state: "running".into(),
            })][..],
            deserialized
                .unwrap()
                .iter()
                .map(to_tuple)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn deserialize_multiple_names_with_and_without_slash() {
        let input = r#"[{"Id":"582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae","Names":["/photoprism-1","photoprism-2"],"NetworkSettings":{"Networks":{}},"Labels": {},"State":"running"}]"#;

        let deserialized: Result<Vec<Container>, _> = serde_json::from_slice(input.as_bytes());

        assert!(deserialized.is_ok());

        assert_eq!(
            &[to_tuple(&Container {
                id: "582036c7a5e8719bbbc9476e4216bfaf4fd318b61723f41f2e8fe3b60d8182ae".into(),
                names: ["photoprism-1".into(), "photoprism-2".into()].into(),
                network_settings: ContainerNetworkSettings {
                    networks: HashMap::new(),
                },
                state: "running".into(),
            })][..],
            deserialized
                .unwrap()
                .iter()
                .map(to_tuple)
                .collect::<Vec<_>>()
        );
    }
}
