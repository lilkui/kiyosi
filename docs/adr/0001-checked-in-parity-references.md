# Checked-in parity references

Parity is evaluated against executable outputs exported from the pinned DerivaSharp revision, but the resulting complete inputs, outputs, provenance, tolerances, and statistical budgets are checked into Kiyosi as language-neutral fixtures. This keeps C++ and Linux tests reproducible without requiring the external .NET checkout; the exporter remains a developer-side regeneration step and must fail when the pinned revision does not match.
