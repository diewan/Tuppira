# Dependency audit posture

The data-plane workspace uses SQLx with `default-features = false` and only
the SQLite runtime features. `sqlx-mysql` remains recorded as an optional
package in Cargo.lock, which makes cargo-audit report RUSTSEC-2023-0071 even
though `cargo tree --target all -i sqlx-mysql` has no resolved path.

The certification audit therefore invokes cargo-audit with that one advisory
explicitly ignored. This is not an approval to enable a MySQL feature. Remove
the exception if SQLx changes its lockfile layout, or revisit it before any
database feature change.
