//! Trusted host access to component storage without running fsync on Tokio.
use capntproto::storage::{
    worker::{Config, Format, Options, ShutdownMode, Worker},
    ComponentId, ComponentUpdate, Limits, ObjectKey, Revision,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let owner = Worker::open(
        dir.path().join("objects"),
        Format::Components,
        Limits::default(),
        Config::default(),
    )
    .await?;
    let client = owner.client(7); // Trusted host-selected quota identity.
    let object = ObjectKey::new(1);
    let count = ComponentId::new(1);
    let label = ComponentId::new(2);
    let options = Options::default();
    let pending = client.try_edit_components(
        object,
        Revision::INITIAL,
        Some(Revision::INITIAL),
        &[
            ComponentUpdate {
                id: count,
                value: Some(&1u64.to_le_bytes()),
            },
            ComponentUpdate {
                id: label,
                value: Some(b"example"),
            },
        ],
        options,
    )?; // Admission is immediate and bounded. The payload has now been copied.
    let revision = pending.await?; // Only a completed durable commit returns success.
    let snapshot = client.try_get_components(object, options)?.await?;
    assert_eq!(snapshot.get(count), Some(1u64.to_le_bytes().as_slice()));
    println!(
        "Published revision {} through the storage owner",
        revision.get()
    );
    drop(snapshot); // Snapshots retain file locks independently of the owner.
    let report = owner.shutdown(ShutdownMode::Drain).wait().await;
    assert!(!report.degraded);
    Ok(())
}
