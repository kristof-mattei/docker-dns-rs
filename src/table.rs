use std::{
    collections::{btree_map::Entry, BTreeMap, HashMap, HashSet},
    fmt::Display,
    hash::Hash,
    net::IpAddr,
    ops::DerefMut,
    sync::{Arc, Mutex, MutexGuard},
};

use hickory_server::{
    authority,
    proto::rr::{RData, Record, RecordSet, RecordType, RrKey},
    store::in_memory::InMemoryAuthority,
};
use ipnet::IpNet;
use regex::Regex;
use tracing::{event, Level};

pub struct AuthorityWrapper {
    authority: Arc<InMemoryAuthority>,
    network_blacklist: Vec<IpNet>,
}

#[derive(PartialEq, Eq, Hash, Debug)]
struct Key(String);

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn foobar<T: std::fmt::Display>(hash_set: &HashSet<T>) -> String {
    hash_set.iter().fold(String::new(), |acc, curr| {
        if acc.is_empty() {
            format!("{}", curr)
        } else {
            format!("{}, {}", acc, curr)
        }
    })
}

impl AuthorityWrapper {
    pub async fn new(
        authority: Arc<InMemoryAuthority>,
        records: Vec<(String, IpAddr)>,
        network_blacklist: Vec<IpNet>,
    ) -> Result<Self, color_eyre::Report> {
        let table = Self {
            authority,
            network_blacklist,
        };

        // for (name, address) in records {
        //     let (record_type, data) = match address {
        //         IpAddr::V4(v4) => (RecordType::A, RData::A(v4.into())),
        //         IpAddr::V6(v6) => (RecordType::AAAA, RData::AAAA(v6.into())),
        //     };

        //     let record = Record::with(name.parse()?, record_type, 0)
        //         .set_data(Some(data))
        //         .clone();

        //     imo.upsert(record, 0).await;
        // }

        for record in records {
            table.add(record.0, record.1).await?;
        }

        Ok(table)
    }

    fn key(&self, name: &str) -> Key {
        Key(name.to_string())
    }

    async fn upsert(
        // storage: &mut impl DerefMut<Target = BTreeMap<RrKey, Arc<RecordSet>>>,
        &self,
        key: String,
        value: String,
    ) -> Result<(), color_eyre::Report> {
        // ???? upsert on authority directly?
        let record = Record::from_rdata(key.parse()?, 0, RData::A(value.parse()?));

        self.authority.upsert(record, 0).await;

        // match storage.entry(key) {
        //     Entry::Occupied(o) => {
        //         // let x = o.get_mut().insert(value.parse()?, 0);

        //         // OLD
        //         //         occupied.get_mut().insert(value);
        //     },
        //     Entry::Vacant(v) => {
        //         // v.insert(Record::with(&mut self, rdata));

        //         // OLD
        //         //         empty.insert(HashSet::from_iter([value]));
        //     },
        // }

        Ok(())
    }

    fn build_reversed(address: &IpAddr) -> String {
        let reversed = address
            .to_string()
            .split('.')
            .rev()
            .collect::<Vec<&str>>()
            .join(".");

        format!("{}.in-addr.arpa", reversed)
    }

    pub async fn add_range(
        &self,
        name_to_address: Vec<(String, IpAddr)>,
    ) -> Result<(), color_eyre::Report> {
        // let Ok(mut storage) = self.storage.lock() else {
        //     event!(Level::ERROR, "Table Mutex poisoned");
        //     return Err(color_eyre::Report::msg("Table Mutex poisoned"));
        // };

        'outer: for (mut name, address) in name_to_address {
            if name.starts_with('.') {
                name = format!("*{}", name);
            }

            let key = self.key(&name);

            // check blacklist...
            for network in &self.network_blacklist {
                if network.contains(&address) {
                    event!(
                        Level::INFO,
                        "skipping table.add {} -> {} (blacklisted network)",
                        name,
                        address,
                    );

                    continue 'outer;
                }
            }

            // reverse map for PTR records
            // let ptr_address = AuthorityWrapper::build_reversed(&address);
            // let ptr_key = self.key(&ptr_address);

            if let Err(e) = self.upsert(key.0, address.to_string()).await {
                event!(Level::ERROR, ?e, "table.add {} -> {}", name, address);
            } else {
                event!(Level::INFO, "table.add {} -> {}", name, address);
            }

            // // TODO using `name` as value here seems weird
            // // AuthorityWrapper::upsert(&mut l, ptr_key, name.clone());
            // event!(Level::INFO, "table.add {} -> {}", ptr_address, name);
        }

        Ok(())
    }

    pub async fn add(&self, name: String, address: IpAddr) -> Result<(), color_eyre::Report> {
        self.add_range(vec![(name, address)]).await
    }

    fn get(self, name: &str) -> Result<HashSet<String>, color_eyre::Report> {
        let key = self.key(name);

        let guard = self.get_guard()?;

        if let Some(result) = guard.get(&key) {
            event!(Level::INFO, "table.get {} with {}", name, foobar(result));

            return Ok(result.clone());
        }

        let wild = Regex::new(r"^[\.]+")
            .unwrap()
            .replace_all(name, "")
            .to_string();

        let wild_key = self.key(&wild);

        if let Some(result) = guard.get(&wild_key) {
            event!(Level::INFO, "table.get {} with {}", name, foobar(result));

            return Ok(result.clone());
        }

        event!(Level::INFO, "table.get {} with no results", name);

        // TODO should this be None?
        Ok(HashSet::new())
    }

    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<(), color_eyre::Report> {
        let old_key = self.key(old_name.strip_prefix('/').unwrap_or(old_name));
        let new_key = self.key(new_name);

        let mut guard = self.get_guard()?;

        if let Some(v) = guard.remove(&old_key) {
            guard.insert(new_key, v);
            event!(Level::INFO, "table.rename ({} -> {})", old_name, new_name);
        } else {
            event!(
                Level::ERROR,
                "table.rename ({} -> {}), entry not found",
                old_name,
                new_name
            );
        }

        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<(), color_eyre::Report> {
        let key = self.key(name);

        let mut storage: MutexGuard<'_, HashMap<Key, HashSet<String>>> = self.get_guard()?;

        // we delete the incoming name -> ip from our storage
        if let Some(addresses) = storage.remove(&key) {
            event!(
                Level::INFO,
                "table.remove {} -> {}",
                name,
                foobar(&addresses)
            );

            // and for each ip in name -> ip we delete the PTR record
            for address in addresses {
                let ptr_address = AuthorityWrapper::build_reversed(
                    &address.parse().expect("Expected IP Address"),
                );
                let ptr_key = self.key(&ptr_address);

                // let raw_entry_builder = storage.raw_entry_mut();

                // match raw_entry_builder.from_key(&ptr_key) {
                //     RawEntryMut::Occupied(mut o) => {
                //         let targets = o.get_mut();
                //         targets.remove(name);

                //         event!(Level::INFO, "table.remove {} -> {}", ptr_key, name);

                //         if targets.is_empty() {
                //             o.remove();

                //             event!(Level::INFO, "table.remove {} as it is empty", ptr_key);
                //         }
                //     },
                //     RawEntryMut::Vacant(_) => {
                //         event!(
                //             Level::WARN,
                //             "table.remove {} -> {} failed, PTR record not found",
                //             ptr_key,
                //             name
                //         );
                //     },
                // }

                // match storage.raw_entry_mut(ptr_key) {
                //     Entry::Occupied(mut o) => {
                //         o.get_mut().remove(name);
                //     },
                //     Entry::Vacant(_) => {
                //     },
                // }

                // if let Some(v) = storage.get_mut(&ptr_key) {
                //     if v.remove(name) {
                //         event!(Level::INFO, "table.remove {} -> {}", ptr_key, name);
                //     } else {
                //     }
                // }
            }
        } else {
            event!(Level::ERROR, "table.remove {}, entry not found", name);
        }

        Ok(())
    }

    fn get_guard(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<Key, HashSet<String>>>, color_eyre::Report> {
        // if let Ok(storage) = self.storage.lock() {
        //     Ok(storage)
        // } else {
        //     event!(Level::ERROR, "Table Mutex poisoned");
        //     Err(color_eyre::Report::msg("Table Mutex poisoned"))
        // }

        todo!()
    }
}

//     def _key(self, name: str):
//         try:
//             label = DNSLabel(name.lower()).label
//             log("LAAAAAAAAAAAAAAAAAAAAAAAA")
//             log(label)
//             log("/LAAAAAAAAAAAAAAAAAAAAAAAA")
//             return label
//         except Exception:
//             return None
