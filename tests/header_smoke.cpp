#include <ito/core/types.hpp>
#include <ito/instruments/barrier.hpp>
#include <ito/instruments/digital.hpp>
#include <ito/instruments/vanilla.hpp>
#include <ito/market/calendar.hpp>
#include <ito/market/context.hpp>
#include <ito/pricing/engines/analytic.hpp>
#include <ito/pricing/engines/barrier.hpp>
#include <ito/pricing/engines/binomial.hpp>
#include <ito/pricing/engines/digital.hpp>
#include <ito/pricing/engines/finite_difference.hpp>
#include <ito/pricing/result.hpp>

static_assert(ito::version_major == 0);
static_assert(ito::PricingRequest::price_only().requests(ito::risk_measure::price));
