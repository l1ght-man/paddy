//! Fill a vault with fake entries for testing and profiling.
//!   cargo run -p paddy-core --example seed -- <vault.db> [count]
use paddy_core::{Entry, Field, Vault};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: seed <vault.db> [count]")?;
    let count: usize = args.next().map(|n| n.parse()).transpose()?.unwrap_or(500);

    let mut vault =
        if std::path::Path::new(&path).exists() { Vault::open(&path)? } else { Vault::create(&path, "seeded")? };
    for i in 0..count {
        let mut e = Entry::new(format!("host-{i:04}"));
        e.tags = vec![["htb", "oscp", "lab", "prod"][i % 4].into(), format!("net{}", i % 16)];
        e.notes = format!("# host {i}\nfake entry for testing\nsee also host-{:04}", (i * 7) % count.max(1));
        e.fields = vec![
            Field::new("host", format!("10.{}.{}.{}", i % 250, (i / 250) % 250, i % 254 + 1)),
            Field::new("user", "administrator"),
            Field::secret("password", format!("P@ss-{i:04}-w0rd")),
            Field::new("port", "22"),
        ];
        vault.add_entry(&e)?;
    }
    vault.save()?;
    println!("{}: {} entries", path, vault.list_entries()?.len());
    Ok(())
}
