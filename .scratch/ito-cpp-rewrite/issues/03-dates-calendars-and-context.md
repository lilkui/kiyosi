Type: task
Status: ready-for-agent
Blocked by: 02

# Dates, schedules, calendars, and valuation context

## Goal

Implement `std::chrono::sys_days`-based date handling, an owning type-erased trading calendar, schedule validation, and the concrete BSM valuation context.

## Depends on

Domain values and validated factories.

## Acceptance criteria

- Date ordering and day arithmetic use `std::chrono`.
- The calendar value owns custom trading-day behavior and its annual trading-day count.
- Built-in all-days and exchange-style calendars cover the initial examples.
- Schedule validation rejects dates outside the instrument life or outside the supplied calendar.
- The valuation context contains BSM parameters, asset price, valuation date, and calendar without generic model templates.
- Calendar copies remain valid without dangling references.

## Test

Test date arithmetic, all-days behavior, custom calendar behavior, schedule boundaries, invalid observation dates, and context copying.
