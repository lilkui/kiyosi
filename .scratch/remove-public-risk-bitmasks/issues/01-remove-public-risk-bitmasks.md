# 01: Remove public risk-measure bitmasks

**What to build:** Simplify pricing so callers invoke each engine without selecting risk measures and receive every result that engine naturally produces. Result availability remains represented by optional risk-measure slots, while internal calculations may retain a private price-only path where useful. Clarify the architectural decision so the documented public contract and implementation agree.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Public pricing entry points no longer accept a risk-measure selection request.
- [ ] Public risk-measure bitmask types, helpers, and engine capability masks are removed.
- [ ] Each engine returns all risk measures it naturally computes, with unavailable measures represented by empty optional slots.
- [ ] Internal implied-volatility and barrier calculations retain any required price-only path without exposing it publicly.
- [ ] The architectural decision explicitly states that risk-measure bitmasks are not part of the public contract.
- [ ] Relevant build and test checks pass.
