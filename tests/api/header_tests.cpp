#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/core/version.hpp>
#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/market/schedule.hpp>
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
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

#include <concepts>

namespace {

template <typename Engine, typename Option>
concept can_price = requires(
    const Engine& engine, const Option& option, const kiyosi::PricingContext& context) {
    { engine.price(option, context) } -> std::same_as<kiyosi::result<kiyosi::PricingResult>>;
};

template <typename Engine, typename Option, typename Settings>
concept can_price_with_settings = requires(
    const Engine& engine, const Option& option, const kiyosi::PricingContext& context) {
    engine.price(option, context, Settings{});
};

} // namespace

static_assert(kiyosi::version_major == 0);

static_assert(can_price<kiyosi::AnalyticVanillaEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::AnalyticVanillaEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::AnalyticVanillaEngine, kiyosi::EuropeanCashOrNothingOption>);
static_assert(can_price<kiyosi::IntegralVanillaEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::IntegralVanillaEngine, kiyosi::AmericanOption>);
static_assert(can_price<kiyosi::BjerksundStenslandVanillaEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::BjerksundStenslandVanillaEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::AnalyticDigitalEngine, kiyosi::EuropeanCashOrNothingOption>);
static_assert(can_price<kiyosi::AnalyticDigitalEngine, kiyosi::EuropeanAssetOrNothingOption>);
static_assert(!can_price<kiyosi::AnalyticDigitalEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::FiniteDifferenceVanillaEngine, kiyosi::EuropeanOption>);
static_assert(can_price<kiyosi::FiniteDifferenceVanillaEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::FiniteDifferenceVanillaEngine, kiyosi::BermudanOption>);

static_assert(can_price<kiyosi::FiniteDifferenceDigitalEngine, kiyosi::EuropeanCashOrNothingOption>);
static_assert(can_price<kiyosi::FiniteDifferenceDigitalEngine, kiyosi::EuropeanAssetOrNothingOption>);
static_assert(!can_price<kiyosi::FiniteDifferenceDigitalEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::CrrVanillaEngine, kiyosi::AmericanOption>);
static_assert(can_price<kiyosi::CrrVanillaEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::CrrVanillaEngine, kiyosi::BermudanOption>);

static_assert(can_price<kiyosi::AnalyticBarrierEngine, kiyosi::BarrierOption>);
static_assert(!can_price<kiyosi::AnalyticBarrierEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::MonteCarloVanillaEngine, kiyosi::EuropeanOption>);
static_assert(can_price<kiyosi::MonteCarloVanillaEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::MonteCarloVanillaEngine, kiyosi::BermudanOption>);

static_assert(!can_price_with_settings<kiyosi::FiniteDifferenceVanillaEngine,
                                       kiyosi::EuropeanOption,
                                       kiyosi::FiniteDifferenceSettings>);
static_assert(!can_price_with_settings<kiyosi::FiniteDifferenceVanillaEngine,
                                       kiyosi::AmericanOption,
                                       kiyosi::FiniteDifferenceSettings>);
static_assert(!can_price_with_settings<kiyosi::CrrVanillaEngine,
                                       kiyosi::AmericanOption,
                                       kiyosi::BinomialSettings>);
