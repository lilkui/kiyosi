#include <kiyosi/core/types.hpp>
#include <kiyosi/instruments/barrier.hpp>
#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/engines/analytic.hpp>
#include <kiyosi/pricing/engines/barrier.hpp>
#include <kiyosi/pricing/engines/binomial.hpp>
#include <kiyosi/pricing/engines/digital.hpp>
#include <kiyosi/pricing/engines/finite_difference.hpp>
#include <kiyosi/pricing/result.hpp>

static_assert(kiyosi::version_major == 0);
static_assert(kiyosi::PricingRequest::price_only().requests(kiyosi::risk_measure::price));
