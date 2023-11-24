use std::net::IpAddr;
use std::sync::Arc;

use color_eyre::eyre::Report;
use hickory_server::proto::rr::{Name, RData, Record, RecordType, RrKey};
use hickory_server::store::in_memory::InMemoryAuthority;
use ipnet::IpNet;
use tracing::{event, Level};

pub struct AuthorityWrapper {
    authority: Arc<InMemoryAuthority>,
    network_blacklist: Vec<IpNet>,
}

fn pretty_print_vec<T: std::fmt::Display>(iterable: impl Iterator<Item = T>) -> String {
    iterable.fold(String::new(), |acc, curr| {
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
    ) -> Result<Self, Report> {
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

    async fn upsert(
        // storage: &mut impl DerefMut<Target = BTreeMap<RrKey, Arc<RecordSet>>>,
        &self,
        name: Name,
        value: String,
    ) -> Result<(), Report> {
        let record = Record::from_rdata(name, 0, RData::A(value.parse()?));

        if self.authority.upsert(record, 0).await {
            Ok(())
        } else {
            Err(Report::msg("Record not updated / inserted, check logs"))
        }
    }

    // fn build_reversed(address: &IpAddr) -> String {
    //     let reversed = address
    //         .to_string()
    //         .split('.')
    //         .rev()
    //         .collect::<Vec<&str>>()
    //         .join(".");

    //     format!("{}.in-addr.arpa", reversed)
    // }

    pub async fn add(&self, mut name: String, address: IpAddr) -> Result<(), Report> {
        // check blacklist...
        for network in &self.network_blacklist {
            if network.contains(&address) {
                event!(
                    Level::INFO,
                    "skipping table.add {} -> {} (blacklisted network)",
                    name,
                    address,
                );

                return Err(Report::msg("Blacklisted"));
            }
        }

        if name.starts_with('.') {
            name = format!("*{}", name);
        }

        let parsed_name: Name = match name.parse() {
            Ok(parsed) => parsed,
            Err(e) => {
                event!(Level::ERROR, ?e, "table.add {} -> {}", name, address);

                return Err(e.into());
            },
        };

        // reverse map for PTR records
        // let ptr_address = AuthorityWrapper::build_reversed(&address);
        // let ptr_key = self.key(&ptr_address);

        if let Err(e) = self.upsert(parsed_name.clone(), address.to_string()).await {
            event!(Level::ERROR, ?e, "table.add {} -> {}", name, address);
            Err(e)
        } else {
            event!(Level::INFO, "table.add {} -> {}", name, address);
            Ok(())
        }

        // // TODO using `name` as value here seems weird
        // // AuthorityWrapper::upsert(&mut l, ptr_key, name.clone());
        // event!(Level::INFO, "table.add {} -> {}", ptr_address, name);
    }

    // fn get(self, name: &str) -> Result<HashSet<String>, color_eyre::Report> {
    //     let key = self.key(name);

    //     let guard = self.get_guard()?;

    //     if let Some(result) = guard.get(&key) {
    //         event!(Level::INFO, "table.get {} with {}", name, foobar(result));

    //         return Ok(result.clone());
    //     }

    //     let wild = Regex::new(r"^[\.]+")
    //         .unwrap()
    //         .replace_all(name, "")
    //         .to_string();

    //     let wild_key = self.key(&wild);

    //     if let Some(result) = guard.get(&wild_key) {
    //         event!(Level::INFO, "table.get {} with {}", name, foobar(result));

    //         return Ok(result.clone());
    //     }

    //     event!(Level::INFO, "table.get {} with no results", name);

    //     // TODO should this be None?
    //     Ok(HashSet::new())
    // }

    pub async fn rename(&self, old_name: &str, new_name: &str) -> Result<(), Report> {
        let old_key = old_name.strip_prefix('/').unwrap_or(old_name).parse()?;
        let new_key = new_name.parse()?;

        let mut records = self.authority.records_mut().await;

        if let Some(v) = records.remove(&RrKey::new(old_key, RecordType::A)) {
            records.insert(RrKey::new(new_key, RecordType::A), v);
            event!(Level::INFO, "table.rename {} -> {}", old_name, new_name);
        } else {
            event!(
                Level::ERROR,
                "table.rename {} -> {}, entry not found",
                old_name,
                new_name
            );
        }

        Ok(())
    }

    pub async fn remove(&self, name: &str) -> Result<(), Report> {
        let mut records = self.authority.records_mut().await;

        let rrkey = RrKey::new(name.parse()?, RecordType::A);

        // we delete the incoming name -> ip from our storage
        if let Some(record_set) = records.remove(&rrkey) {
            let names = record_set
                .records_without_rrsigs()
                .filter_map(|record| record.data().map(ToString::to_string))
                .collect::<Vec<_>>();

            event!(
                Level::INFO,
                "table.remove {} -> {}",
                name,
                pretty_print_vec(names.iter())
            );

            // and for each ip in name -> ip we delete the PTR record
            // for address in addresses {
            //     let ptr_address = AuthorityWrapper::build_reversed(
            //         &address.parse().expect("Expected IP Address"),
            //     );
            //     let ptr_key = self.key(&ptr_address);

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
            // }
        } else {
            event!(Level::ERROR, "table.remove {}, entry not found", name);
        }

        Ok(())
    }
}
