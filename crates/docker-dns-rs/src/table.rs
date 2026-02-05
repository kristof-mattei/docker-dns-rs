use std::net::IpAddr;
use std::sync::Arc;

use color_eyre::eyre::Report;
use hickory_server::proto::rr::rdata::PTR;
use hickory_server::proto::rr::{Name, RData, RecordSet, RecordType, RrKey};
use hickory_server::store::in_memory::InMemoryAuthority;
use tracing::{Level, event};

pub struct AuthorityWrapper {
    authority: Arc<InMemoryAuthority>,
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
    pub fn new(authority: Arc<InMemoryAuthority>) -> Self {
        Self { authority }
    }

    async fn upsert(&self, name: Name, address: IpAddr) {
        // reverse map for PTR records
        let reverse: Name = address.into();

        let reverse_set = {
            let rdata = RData::PTR(PTR(name.clone()));

            let mut set = RecordSet::with_ttl(reverse.clone(), rdata.record_type(), 5);
            // false means identical data was found, which is fine for us
            set.add_rdata(rdata);

            set
        };

        let a_or_aaaa_set = {
            let rdata: RData = address.into();

            let mut set = RecordSet::with_ttl(name.clone(), rdata.record_type(), 5);
            // false means identical data was found, which is fine for us
            set.add_rdata(rdata);

            set
        };

        let mut lock = self.authority.records_mut().await;

        lock.insert(
            RrKey::new(name.into(), a_or_aaaa_set.record_type()),
            a_or_aaaa_set.into(),
        );
        lock.insert(
            RrKey::new(reverse.into(), reverse_set.record_type()),
            reverse_set.into(),
        );
    }

    pub async fn add(&self, name: Name, address: IpAddr) -> Result<(), (Name, Report)> {
        self.upsert(name.clone(), address).await;

        event!(Level::INFO, "table.add {} -> {}", name, address);
        Ok(())

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
        let old_key = RrKey::new(
            old_name.strip_prefix('/').unwrap_or(old_name).parse()?,
            RecordType::A,
        );
        let new_name_parsed: Name = new_name.parse()?;

        let mut records = self.authority.records().await;

        let mut ips: Vec<IpAddr> = vec![];

        if let Some(v) = records.remove(&old_key) {
            for record in v.records_without_rrsigs() {
                let data = record.data();

                if let Some(a) = data.as_a() {
                    ips.push(a.0.into());
                } else if let Some(aaaa) = data.as_aaaa() {
                    ips.push(aaaa.0.into());
                } else {
                    // we only care about A & AAAA
                }
            }
        } else {
            event!(
                Level::ERROR,
                "table.rename {} -> {}, entry not found",
                old_name,
                new_name
            );
        }

        for ip in ips {
            self.upsert(new_name_parsed.clone(), ip).await;

            event!(
                Level::INFO,
                "table.rename {} -> {} ({})",
                old_name,
                new_name,
                ip
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
                .filter_map(|record| record.data().as_a())
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
