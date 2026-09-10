# Test plan

1. Extend the existing generated-reference test at its public pricing seam, first
   requiring numerical rows so absence fails. Reuse the analytic reference stability checks.
2. Add declared numerical profiles and generated matrix rows; compare native contracts
   and all wrapper measures for ATM one-year call/put cases. Serialize shifts explicitly.
3. Recompute five legacy prices and three convergence targets; retain IDs and settings
   so existing price, convergence and repeatability checks remain active.
4. Extend standalone generation checks for numerical profiles and retained data.
5. Build and run the reference tests, diagnose discrepancies, then frozen repeatability
   and full C++ suite. Review standards/spec separately before Conventional Commit.
