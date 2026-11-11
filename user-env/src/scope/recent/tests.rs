use super::*;
use du_core_contracts::settings::SETTINGS_SCHEMA_VERSION;

impl RecentBody for String {
    fn body(&self) -> &str {
        self
    }
}

#[derive(Default)]
struct Fake {
    writes: usize,
}
impl RecentRepository for Fake {
    type Summary = ();
    type Item = ();
    type Write = String;
    fn list(&mut self, _: NonZeroUsize) -> Result<Vec<()>, RepositoryError> {
        Ok(vec![])
    }
    fn get(
        &mut self,
        _: &[u8; 16],
        _: NonZeroUsize,
        _: &dyn Fn(&str) -> bool,
    ) -> Result<Option<()>, RepositoryError> {
        Ok(None)
    }
    fn put(
        &mut self,
        _: &String,
        _: NonZeroUsize,
        _: &dyn Fn(&str) -> bool,
    ) -> Result<(), RepositoryError> {
        self.writes += 1;
        Ok(())
    }
}

#[test]
fn snapshot_serialization_is_the_callers_responsibility() -> Result<(), Box<dyn std::error::Error>>
{
    let mut current = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    };
    let stale_on = current.clone();
    current.values.insert(
        super::super::REMEMBER_RECENT.into(),
        serde_json::json!(false),
    );
    let cap = NonZeroUsize::new(1).ok_or("zero fixture cap")?;
    let mut service = RecentService::new(Fake::default(), cap, |_: &str| true);
    let item = "fixture".to_owned();
    assert_eq!(service.put(Some(&current), &item), Ok(()));
    assert_eq!(service.repository.writes, 0);
    // Passing an older on snapshot cannot enforce a later off transition.
    assert_eq!(service.put(Some(&stale_on), &item), Ok(()));
    assert_eq!(service.repository.writes, 1);
    Ok(())
}
