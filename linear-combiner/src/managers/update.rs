use rocksdb::{IteratorMode, WriteBatch, DB};

use crate::{error::LcError, item::LtItem};

#[derive(Debug)]
pub struct UpdateManager;

impl UpdateManager {
	pub fn set_value(db: &DB, key: Vec<u8>, value: f32) -> Result<(), LcError> {
		let cf = db.cf_handle("update").ok_or_else(|| LcError::NotFoundError)?;
		db.put_cf(&cf, key.clone(), value.to_be_bytes()).map_err(|e| LcError::DbError(e))?;
		Ok(())
	}

	pub fn read_batch(db: &DB, prefix: Vec<u8>, n: u32) -> Result<Vec<LtItem>, LcError> {
		let cf = db.cf_handle("update").ok_or_else(|| LcError::NotFoundError)?;
		let mut iter = db.prefix_iterator_cf(&cf, prefix);
		iter.set_mode(IteratorMode::Start);

		let size = usize::try_from(n).map_err(|_| LcError::ParseError)?;
		let items = iter.take(size).try_fold(Vec::new(), |mut acc, item| {
			item.map(|(key, value)| {
				let lt_item = LtItem::from_raw(key, value);
				acc.push(lt_item);
				acc
			})
			.map_err(|e| LcError::DbError(e))
		});

		items
	}

	pub fn delete_batch(db: &DB, prefix: Vec<u8>, items: Vec<LtItem>) -> Result<(), LcError> {
		let cf = db.cf_handle("update").ok_or_else(|| LcError::NotFoundError)?;
		let mut batch = WriteBatch::default();
		items.iter().for_each(|x| {
			let mut key = Vec::new();
			key.extend_from_slice(&prefix);
			key.extend_from_slice(&x.key_bytes());
			batch.delete_cf(&cf, key);
		});
		db.write(batch).map_err(|e| LcError::DbError(e))?;
		Ok(())
	}
}

#[cfg(test)]
mod test {
	use crate::{item::LtItem, managers::update::UpdateManager};
	use rocksdb::{Options, DB};

	#[test]
	fn should_read_delete_batch() {
		let mut opts = Options::default();
		opts.create_missing_column_families(true);
		opts.create_if_missing(true);
		let db = DB::open_cf(&opts, "lc-rdb-test-storage", vec!["update"]).unwrap();

		let prefix = vec![0; 8];
		let key = vec![0; 16];
		let weight = 50.;

		UpdateManager::set_value(&db, key.clone(), weight).unwrap();

		let org_items = vec![LtItem::from_raw(key.clone(), weight.to_be_bytes().to_vec())];
		let items = UpdateManager::read_batch(&db, prefix.clone(), 1).unwrap();
		assert_eq!(items, org_items);

		UpdateManager::delete_batch(&db, prefix.clone(), items).unwrap();
		let items = UpdateManager::read_batch(&db, prefix, 1).unwrap();
		assert_eq!(items, Vec::new());
	}
}
