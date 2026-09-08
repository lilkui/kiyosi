# Deliberate parity exceptions

Scheduled barriers use contractual observation-date state rather than treating a non-observation valuation-date crossing as historical touch; the BGK approximation remains between observations. American-put validation uses an independent correct Bjerksund–Stensland reference instead of reproducing DerivaSharp's known transformed-put defect. These are documented parity exceptions because correctness takes precedence over copying the pinned implementation's boundary behavior.
