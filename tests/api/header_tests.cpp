#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/core/version.hpp>
#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
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
#include <kiyosi/pricing/settings/binomial.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/implied.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

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

static_assert(kiyosi::default_realized_average == 0.0);
static_assert(kiyosi::BarrierOptionTerms{}.rebate == 0.0);
static_assert(kiyosi::BarrierOptionTerms{}.rebate_timing == kiyosi::rebate_timing::at_expiry);
static_assert(kiyosi::BarrierOptionTerms{}.observation_mode == kiyosi::observation_mode::continuous);
static_assert(kiyosi::BinaryBarrierTerms{}.observation_mode == kiyosi::observation_mode::continuous);
static_assert(kiyosi::settlement_timing::at_expiry != kiyosi::settlement_timing::at_hit);
static_assert(kiyosi::AccumulatorTerms{}.accumulated_quantity == 0.0);
static_assert(kiyosi::SnowballTerms{}.touch_status == kiyosi::barrier_touch_status::none);
static_assert(kiyosi::SnowballTerms{}.principal_ratio == 1.0);
static_assert(kiyosi::BinarySnowballTerms{}.touch_status == kiyosi::barrier_touch_status::none);
static_assert(kiyosi::BinarySnowballTerms{}.principal_ratio == 1.0);
static_assert(kiyosi::TernarySnowballTerms{}.touch_status == kiyosi::barrier_touch_status::none);
static_assert(kiyosi::TernarySnowballTerms{}.principal_ratio == 1.0);
static_assert(kiyosi::PhoenixTerms{}.touch_status == kiyosi::barrier_touch_status::none);
static_assert(kiyosi::PhoenixTerms{}.principal_ratio == 1.0);

static_assert(kiyosi::FiniteDifferenceSettings{}.asset_steps == 200);
static_assert(kiyosi::FiniteDifferenceSettings{}.time_steps == 200);
static_assert(kiyosi::FiniteDifferenceSettings{}.scheme ==
              kiyosi::finite_difference_scheme::crank_nicolson);
static_assert(kiyosi::StructuredMonteCarloSettings{}.path_count == 20'000);
static_assert(kiyosi::StructuredMonteCarloSettings{}.seed == 1);
static_assert(kiyosi::MonteCarloSettings{}.path_count == 100'000);
static_assert(kiyosi::MonteCarloSettings{}.step_count == 50);
static_assert(!kiyosi::MonteCarloSettings{}.seed);
static_assert(kiyosi::MonteCarloSettings{}.backend == kiyosi::monte_carlo_backend::cpu);
static_assert(kiyosi::monte_carlo_backend::cpu != kiyosi::monte_carlo_backend::cuda);
static_assert(kiyosi::NumericalShiftSettings{}.spot_shift == 1e-2);
static_assert(kiyosi::NumericalShiftSettings{}.volatility_shift == 1e-4);
static_assert(kiyosi::NumericalShiftSettings{}.rate_shift == 1e-4);
static_assert(kiyosi::NumericalShiftSettings{}.time_shift_days == 1);
static_assert(kiyosi::ImpliedVolatilitySettings{}.lower_bound == 0.0001);
static_assert(kiyosi::ImpliedVolatilitySettings{}.upper_bound == 4.0);
static_assert(kiyosi::ImpliedVolatilitySettings{}.tolerance == 1e-8);
static_assert(kiyosi::ImpliedVolatilitySettings{}.max_iterations == 100);
static_assert(kiyosi::ImpliedCouponSettings{}.lower_bound == 0.0);
static_assert(kiyosi::ImpliedCouponSettings{}.upper_bound == 2.0);
static_assert(kiyosi::ImpliedCouponSettings{}.tolerance == 1e-8);
static_assert(kiyosi::ImpliedCouponSettings{}.max_iterations == 100);
static_assert(kiyosi::BinomialSettings{}.steps == 256);

static_assert(std::equality_comparable<kiyosi::PhoenixOption>);
static_assert(std::equality_comparable<kiyosi::SnowballOption>);
static_assert(std::equality_comparable<kiyosi::BinarySnowballOption>);
static_assert(std::equality_comparable<kiyosi::TernarySnowballOption>);

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

static_assert(can_price<kiyosi::FiniteDifferenceDigitalEngine, kiyosi::EuropeanCashOrNothingOption>);
static_assert(can_price<kiyosi::FiniteDifferenceDigitalEngine, kiyosi::EuropeanAssetOrNothingOption>);
static_assert(!can_price<kiyosi::FiniteDifferenceDigitalEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::CrrVanillaEngine, kiyosi::AmericanOption>);
static_assert(can_price<kiyosi::CrrVanillaEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::AnalyticBarrierEngine, kiyosi::BarrierOption>);
static_assert(!can_price<kiyosi::AnalyticBarrierEngine, kiyosi::EuropeanOption>);

static_assert(can_price<kiyosi::MonteCarloVanillaEngine, kiyosi::EuropeanOption>);
static_assert(can_price<kiyosi::MonteCarloVanillaEngine, kiyosi::AmericanOption>);

static_assert(!can_price_with_settings<kiyosi::FiniteDifferenceVanillaEngine,
                                       kiyosi::EuropeanOption,
                                       kiyosi::FiniteDifferenceSettings>);
static_assert(!can_price_with_settings<kiyosi::FiniteDifferenceVanillaEngine,
                                       kiyosi::AmericanOption,
                                       kiyosi::FiniteDifferenceSettings>);
static_assert(!can_price_with_settings<kiyosi::CrrVanillaEngine,
                                       kiyosi::AmericanOption,
                                       kiyosi::BinomialSettings>);
