use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::net::IpAddr;
use std::sync::Arc;

use color_eyre::eyre::Report;
use hickory_server::proto::rr::rdata::PTR;
use hickory_server::proto::rr::{LowerName, Name, RData, Record, RecordSet, RecordType, RrKey};
use hickory_server::store::in_memory::InMemoryAuthority;
use tracing::{Level, event, instrument};

pub struct AuthorityWrapper {
    authority: Arc<InMemoryAuthority>,
}

fn append_to_record_set(
    records: &mut BTreeMap<RrKey, Arc<RecordSet>>,
    key: RrKey,
    owner: Name,
    record_type: RecordType,
    rdata: RData,
) {
    match records.entry(key) {
        Entry::Occupied(mut entry) => {
            Arc::make_mut(entry.get_mut()).add_rdata(rdata);
        },
        Entry::Vacant(vacant_entry) => {
            let mut set = RecordSet::with_ttl(owner, record_type, 5);

            set.add_rdata(rdata);

            vacant_entry.insert(Arc::new(set));
        },
    }
}

fn remove_from_ptr_set(
    records: &mut BTreeMap<RrKey, Arc<RecordSet>>,
    ip: IpAddr,
    name: &LowerName,
) {
    let reverse = Name::from(ip);
    let ptr_key = RrKey::new(LowerName::new(&reverse), RecordType::PTR);

    let Entry::Occupied(mut entry) = records.entry(ptr_key) else {
        return;
    };

    // Check existence under the same lock before paying the clone cost.
    let exists = entry
        .get()
        .records_without_rrsigs()
        .any(|r| matches!(r.data(), RData::PTR(PTR(n)) if &LowerName::new(n) == name));

    if !exists {
        return;
    }

    let record_to_remove = Record::from_rdata(reverse, 0, RData::PTR(PTR(Name::from(name))));

    let is_empty = {
        let set = Arc::make_mut(entry.get_mut());
        set.remove(&record_to_remove, 0);
        set.is_empty()
    };

    if is_empty {
        entry.remove();
    }
}

impl AuthorityWrapper {
    pub fn new(authority: Arc<InMemoryAuthority>) -> Self {
        Self { authority }
    }

    async fn upsert(&self, name: &Name, address: IpAddr) {
        let rdata: RData = RData::from(address);
        let record_type = rdata.record_type();
        let reverse: Name = address.into();

        let mut lock = self.authority.records_mut().await;

        append_to_record_set(
            &mut lock,
            RrKey::new(LowerName::new(name), record_type),
            name.clone(),
            record_type,
            rdata,
        );

        append_to_record_set(
            &mut lock,
            RrKey::new(LowerName::new(&reverse), RecordType::PTR),
            reverse,
            RecordType::PTR,
            RData::PTR(PTR(name.clone())),
        );
    }

    pub async fn add(&self, name: &Name, address: IpAddr) {
        self.upsert(name, address).await;

        event!(Level::INFO, %name, %address, "table.add");
    }

    #[instrument(skip_all, fields(old_name = %old_key.name, %new_name, r#type = %old_key.record_type))]
    async fn rename_records(&self, old_key: &RrKey, new_name: &Name) -> Result<(), ()> {
        let ips = {
            let mut records = self.authority.records_mut().await;

            let Some(record_set) = records.remove(old_key) else {
                return Err(());
            };

            let ips: Vec<IpAddr> = record_set
                .records_without_rrsigs()
                .filter_map(|r| r.data().ip_addr())
                .collect();

            for &ip in &ips {
                remove_from_ptr_set(&mut records, ip, &old_key.name);
            }

            ips
        };

        for ip in ips {
            self.upsert(new_name, ip).await;

            event!(Level::INFO, %ip, "table.rename");
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

        Ok(())
    }

    async fn remove_record(&self, name: &Name, ip: IpAddr) -> Result<(), ()> {
        let record_type = RData::from(ip).record_type();
        let key = RrKey::new(LowerName::new(name), record_type);

        let mut records = self.authority.records_mut().await;

        {
            let Entry::Occupied(mut entry) = records.entry(key) else {
                return Err(());
            };

            // Check existence under the same lock before paying the clone cost.
            let exists = entry
                .get()
                .records_without_rrsigs()
                .any(|r| r.data().ip_addr() == Some(ip));

            if !exists {
                return Err(());
            }

            let record_to_remove = Record::from_rdata(name.clone(), 0, RData::from(ip));

            let is_empty = {
                let set = Arc::make_mut(entry.get_mut());
                set.remove(&record_to_remove, 0);
                set.is_empty()
            };

            if is_empty {
                entry.remove();
            }
        }

        remove_from_ptr_set(&mut records, ip, &LowerName::new(name));

        event!(Level::INFO, %name, %ip, "table.remove");

        Ok(())
    }

    pub async fn remove_address(&self, name: &Name, ip: IpAddr) {
        if self.remove_record(name, ip).await.is_err() {
            event!(Level::WARN, %name, %ip, "No record found to remove");
        }
    }
}
