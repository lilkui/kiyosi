#include <kiyosi/core/types.hpp>
#include <kiyosi/instruments/barrier.hpp>
#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/calendar.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/engines/asian/average.hpp>
#include <kiyosi/pricing/engines/barrier/analytic.hpp>
#include <kiyosi/pricing/engines/barrier/binary.hpp>
#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>
#include <kiyosi/pricing/engines/digital/analytic.hpp>
#include <kiyosi/pricing/engines/digital/finite_difference.hpp>
#include <kiyosi/pricing/engines/digital/integral.hpp>
#include <kiyosi/pricing/engines/settings/finite_difference.hpp>
#include <kiyosi/pricing/engines/structured/finite_difference.hpp>
#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>
#include <kiyosi/pricing/engines/vanilla/analytic.hpp>
#include <kiyosi/pricing/engines/vanilla/binomial.hpp>
#include <kiyosi/pricing/engines/vanilla/bjerksund_stensland.hpp>
#include <kiyosi/pricing/engines/vanilla/finite_difference.hpp>
#include <kiyosi/pricing/engines/vanilla/integral.hpp>
#include <kiyosi/pricing/engines/vanilla/monte_carlo.hpp>
#include <kiyosi/pricing/result.hpp>

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

}

static_assert(kiyosi::version_major == 0);

static_assert(can_price<kiyosi::AnalyticEuropeanEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::AnalyticEuropeanEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::AnalyticEuropeanEngine, kiyosi::EuropeanCashOrNothingOption>);

static_assert(can_price<kiyosi::AnalyticDigitalEngine, kiyosi::EuropeanCashOrNothingOption>);
static_assert(can_price<kiyosi::AnalyticDigitalEngine, kiyosi::EuropeanAssetOrNothingOption>);
static_assert(!can_price<kiyosi::AnalyticDigitalEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::AnalyticDigitalEngine, kiyosi::AmericanCashOrNothingOption>);

static_assert(can_price<kiyosi::FiniteDifferenceEuropeanEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::FiniteDifferenceEuropeanEngine, kiyosi::AmericanOption>);
static_assert(can_price<kiyosi::FiniteDifferenceAmericanEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::FiniteDifferenceAmericanEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::BinomialAmericanEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::BinomialAmericanEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::BinomialAmericanEngine, kiyosi::BermudanOption>);

static_assert(can_price<kiyosi::AnalyticBarrierEngine, kiyosi::BarrierOption>);
static_assert(!can_price<kiyosi::AnalyticBarrierEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::MonteCarloEuropeanEngine, kiyosi::EuropeanOption>);
static_assert(!can_price<kiyosi::MonteCarloEuropeanEngine, kiyosi::AmericanOption>);
static_assert(can_price<kiyosi::MonteCarloAmericanEngine, kiyosi::AmericanOption>);
static_assert(!can_price<kiyosi::MonteCarloAmericanEngine, kiyosi::EuropeanOption>);
static_assert(can_price<kiyosi::McEuropeanEngine, kiyosi::EuropeanOption>);

static_assert(!can_price_with_settings<kiyosi::FiniteDifferenceEuropeanEngine,
                                       kiyosi::EuropeanOption,
                                       kiyosi::FiniteDifferenceSettings>);
static_assert(!can_price_with_settings<kiyosi::FiniteDifferenceAmericanEngine,
                                       kiyosi::AmericanOption,
                                       kiyosi::FiniteDifferenceSettings>);
static_assert(!can_price_with_settings<kiyosi::BinomialAmericanEngine,
                                       kiyosi::AmericanOption,
                                       kiyosi::BinomialAmericanSettings>);
