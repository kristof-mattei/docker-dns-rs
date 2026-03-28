use std::net::IpAddr;
use std::sync::Arc;

use color_eyre::eyre::Report;
use hickory_server::proto::rr::rdata::PTR;
use hickory_server::proto::rr::{LowerName, Name, RData, RecordSet, RecordType, RrKey};
use hickory_server::store::in_memory::InMemoryAuthority;
use tracing::{Level, event, instrument};

use crate::utils::pretty_print_iter;

pub struct AuthorityWrapper {
    authority: Arc<InMemoryAuthority>,
}

impl AuthorityWrapper {
    pub fn new(authority: Arc<InMemoryAuthority>) -> Self {
        Self { authority }
    }

    async fn upsert(&self, name: &Name, address: IpAddr) {
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

    pub async fn add(&self, name: &Name, address: IpAddr) {
        self.upsert(name, address).await;

        event!(Level::INFO, %name, %address, "table.add");

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

    #[instrument(skip_all, fields(old_name = %old_key.name, %new_name, r#type = %old_key.record_type))]
    async fn rename_records(&self, old_key: &RrKey, new_name: &Name) -> Result<(), ()> {
        let mut records = self.authority.records_mut().await;

        let mut ips: Vec<IpAddr> = vec![];

        let Some(record_set) = records.remove(old_key) else {
            return Err(());
        };

        drop(records);

        for record in record_set.records_without_rrsigs() {
            let data = record.data();

            if let Some(address) = data.ip_addr() {
                ips.push(address);
            } else {
                // we only care about A & AAAA
            }
        }

        for ip in ips {
            self.upsert(new_name, ip).await;

            event!(
                Level::INFO,
                %ip,
                "table.rename",
            );
        }
        Ok(())
    }

    pub async fn rename(&self, old_name: &str, new_name: &str) -> Result<(), Report> {
        let new_name_parsed: Name = new_name.parse()?;
        let old_key: LowerName = old_name.strip_prefix('/').unwrap_or(old_name).parse()?;

        let old_key_a = RrKey::new(old_key.clone(), RecordType::A);
        let a_result = self.rename_records(&old_key_a, &new_name_parsed).await;

        let old_key_aaaa = RrKey::new(old_key, RecordType::AAAA);
        let aaaa_result = self.rename_records(&old_key_aaaa, &new_name_parsed).await;

        if let (Err(()), Err(())) = (a_result, aaaa_result) {
            event!(
                Level::WARN,
                %old_name,
                %new_name,
                "No A or AAAA records found"
            );
        }

        // this is not fatal
        Ok(())
    }

    #[instrument(skip_all, fields(name = %rrkey.name, r#type = %rrkey.record_type))]
    async fn remove_records(&self, rrkey: &RrKey) -> Result<(), ()> {
        let mut records = self.authority.records_mut().await;

        let Some(record_set) = records.remove(rrkey) else {
            return Err(());
        };

        let ips: Vec<IpAddr> = record_set
            .records_without_rrsigs()
            .filter_map(|record| record.data().ip_addr())
            .collect();

        for &ip in &ips {
            let ptr_key = RrKey::new(Name::from(ip).into(), RecordType::PTR);
            records.remove(&ptr_key);
        }

        drop(records);

        event!(
            Level::INFO,
            ip_addresses = %pretty_print_iter(ips.iter().copied()),
            "table.remove",
        );

        Ok(())
    }

    pub async fn remove(&self, name: &str) -> Result<(), Report> {
        let parsed_name: LowerName = name.parse()?;

        let rrkey_a = RrKey::new(parsed_name.clone(), RecordType::A);
        let a_result = self.remove_records(&rrkey_a).await;

        let rrkey_aaaa = RrKey::new(parsed_name, RecordType::AAAA);
        let aaaa_result = self.remove_records(&rrkey_aaaa).await;

        if let (Err(()), Err(())) = (a_result, aaaa_result) {
            event!(
                Level::WARN,
                %name,
                "No A or AAAA records found"
            );
        }

        Ok(())
    }
}
