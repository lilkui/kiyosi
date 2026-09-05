Type: task
Status: ready-for-agent
Blocked by: 04, 05

# Analytic digital and barrier engines

## Goal

Extend the explicit pricing seam to digital and barrier instruments with analytic value and risk behavior.

## Depends on

European analytic engine and parity fixture harness.

## Acceptance criteria

- Cash-or-nothing and asset-or-nothing instruments validate their contractual terms.
- Analytic digital pricing supports calls and puts under the agreed BSM assumptions.
- Vanilla barrier types, observation rules, rebates, and touch behavior are represented and validated.
- Analytic barrier pricing returns the shared result contract where the method supports it.
- Continuous and scheduled observation semantics are explicit.

## Test

Use fixture cases for each digital/barrier direction, rebate timing, observation mode, expiry, and edge boundary; compare all available outputs within method-specific tolerances.
