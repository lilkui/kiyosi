#pragma once

/// \file
/// Thread safety: Unless documented otherwise, operations on distinct objects and concurrent
/// const operations on the same object are safe. An object must remain alive and must not be
/// moved from, assigned to, or otherwise mutated during concurrent access. Concurrent access
/// involving mutation requires external synchronization.

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/core/version.hpp>

#include <kiyosi/market/bsm_parameters.hpp>
#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/calendars/sse.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/market/schedule.hpp>
#include <kiyosi/market/schedule_builders.hpp>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/instruments/exercise.hpp>
#include <kiyosi/instruments/exercise_based_option.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/payoff.hpp>
#include <kiyosi/instruments/structured/autocallable.hpp>
#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/presets.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/instruments/vanilla.hpp>

#include <kiyosi/pricing/analytics.hpp>
#include <kiyosi/pricing/implied.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/scenario.hpp>

#include <kiyosi/pricing/settings/binomial.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/implied.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

#include <kiyosi/pricing/engines/accumulator/finite_difference.hpp>
#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>
#include <kiyosi/pricing/engines/asian/analytic.hpp>
#include <kiyosi/pricing/engines/barrier/analytic.hpp>
#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>
#include <kiyosi/pricing/engines/binary_barrier/analytic.hpp>
#include <kiyosi/pricing/engines/digital/analytic.hpp>
#include <kiyosi/pricing/engines/digital/finite_difference.hpp>
#include <kiyosi/pricing/engines/digital/integral.hpp>
#include <kiyosi/pricing/engines/structured/finite_difference.hpp>
#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>
#include <kiyosi/pricing/engines/vanilla/analytic.hpp>
#include <kiyosi/pricing/engines/vanilla/binomial.hpp>
#include <kiyosi/pricing/engines/vanilla/bjerksund_stensland.hpp>
#include <kiyosi/pricing/engines/vanilla/finite_difference.hpp>
#include <kiyosi/pricing/engines/vanilla/integral.hpp>
#include <kiyosi/pricing/engines/vanilla/monte_carlo.hpp>
