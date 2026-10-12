#include <chrono>
#include <initializer_list>
#include <string>
#include <utility>
#include <vector>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"

namespace {

auto monte_carlo_scenario()
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    return std::pair{
        *kiyosi::make_european_option(
            kiyosi::OptionType::call, 100.0, effective_date, expiry_date),
        context};
}

auto american_monte_carlo_scenario()
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective_date);
    return std::pair{
        *kiyosi::make_american_option(
            kiyosi::OptionType::put, 100.0, effective_date, expiry_date),
        context};
}

auto accumulator_scenario()
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    return std::pair{
        *kiyosi::make_accumulator({.strike = 100.0,
                                   .knock_out_level = 115.0,
                                   .daily_quantity = 1.0,
                                   .acceleration_factor = 2.0,
                                   .accumulated_quantity = 0.0,
                                   .effective_date = effective_date,
                                   .expiry_date = expiry_date}),
        context};
}

auto phoenix_scenario()
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const std::vector<kiyosi::Date> observations{
        kiyosi::Date{std::chrono::year{2025} / 4 / 1},
        kiyosi::Date{std::chrono::year{2025} / 7 / 1},
        kiyosi::Date{std::chrono::year{2025} / 10 / 1}, expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    return std::pair{
        *kiyosi::make_phoenix_option({.coupon_rate = 0.002,
                                      .initial_spot = 100.0,
                                      .knock_in_level = 75.0,
                                      .knock_out_levels = {110.0, 108.0, 106.0, 104.0},
                                      .coupon_barrier_levels = {90.0, 90.0, 90.0, 90.0},
                                      .upper_strike = 100.0,
                                      .lower_strike = 60.0,
                                      .observation_dates = observations,
                                      .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                      .barrier_state = kiyosi::AutocallableBarrierState::none,
                                      .principal_ratio = 1.0,
                                      .effective_date = effective_date,
                                      .expiry_date = expiry_date}),
        context};
}

auto snowball_scenario(kiyosi::AutocallableBarrierState barrier_state = kiyosi::AutocallableBarrierState::none)
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const std::vector<kiyosi::Date> observations{
        kiyosi::Date{std::chrono::year{2025} / 4 / 1},
        kiyosi::Date{std::chrono::year{2025} / 7 / 1},
        kiyosi::Date{std::chrono::year{2025} / 10 / 1}, expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    return std::pair{
        *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08, 0.08},
                                       .maturity_coupon_rate = 0.06,
                                       .initial_spot = 100.0,
                                       .knock_in_level = 75.0,
                                       .knock_out_levels = {110.0, 108.0, 106.0, 104.0},
                                       .upper_strike = 100.0,
                                       .lower_strike = 60.0,
                                       .observation_dates = observations,
                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                       .barrier_state = barrier_state,
                                       .principal_ratio = 1.0,
                                       .effective_date = effective_date,
                                       .expiry_date = expiry_date}),
        context};
}

void register_monte_carlo(const std::string& name, const auto& scenario, auto engine)
{
    const auto& [option, context] = scenario;
    kiyosi::benchmark_support::register_price(name.c_str(), option, std::move(engine), context)
        ->UseRealTime()
        ->Repetitions(5)
        ->ReportAggregatesOnly(true)
        ->Unit(benchmark::kMillisecond);
}

} // namespace

void kiyosi::benchmark_support::register_analytics_cases()
{
    using namespace kiyosi;
    const auto [european, base_context] = monte_carlo_scenario();
    register_calculation("analytics/kiyosi/european/analytic/delta", [=] { return AnalyticVanillaEngine{}.price_with_greeks(european, base_context, {Greek::delta}); }, "price");
    register_calculation("analytics/kiyosi/european/analytic/rho", [=] { return AnalyticVanillaEngine{}.price_with_greeks(european, base_context, {Greek::rho}); }, "price");
    const auto context = *make_pricing_context(
        base_context.model_parameters(), base_context.spot_price(),
        base_context.valuation_time() + std::chrono::hours{12}, base_context.calendar());
    const auto accumulator = accumulator_scenario().first;
    const auto snowball = snowball_scenario().first;
    const MonteCarloSnowballEngine snowball_monte_carlo{20'000, 42};
    register_price("analytics/kiyosi/snowball/mc/price", snowball, snowball_monte_carlo, context)->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/delta_gamma", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/delta_gamma_unavailable_speed", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma, Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    const FiniteDifferenceAccumulatorEngine accumulator_engine{};
    const MonteCarloAccumulatorEngine accumulator_monte_carlo{20'000, 42};
    register_price("analytics/kiyosi/accumulator/mc/price", accumulator, accumulator_monte_carlo, context)->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/delta_gamma", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/delta_gamma_unavailable_speed", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma, Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/rho", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::rho}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/delta_gamma_rho", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma, Greek::rho}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/all", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, true); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/numerical_all", [=] { return calculate_numerical_greeks(accumulator_monte_carlo, accumulator, context); }, "price")->UseRealTime();
#if KIYOSI_HAS_CUDA
    const MonteCarloAccumulatorEngine cuda_accumulator{250'000, 42, MonteCarloBackend::cuda};
    const MonteCarloSnowballEngine cuda_snowball{250'000, 42, MonteCarloBackend::cuda};
    register_price("analytics/kiyosi/accumulator/mc_cuda/price", accumulator, cuda_accumulator, context)->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc_cuda/delta_gamma", [=] { return cuda_accumulator.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_price("analytics/kiyosi/snowball/mc_cuda/price", snowball, cuda_snowball, context)->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc_cuda/delta_gamma", [=, snowball = snowball] { return cuda_snowball.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    const auto accumulator_quote = cuda_accumulator.price(accumulator, context);
    register_calculation("analytics/kiyosi/accumulator/mc_cuda/implied_volatility", [=, accumulator_quote = accumulator_quote]() -> Result<double> {
        if (!accumulator_quote) return std::unexpected(accumulator_quote.error());
        return implied_volatility(cuda_accumulator, accumulator, context, *accumulator_quote); }, "volatility")->UseRealTime();
    const auto snowball_quote = cuda_snowball.price(snowball, context);
    register_calculation("analytics/kiyosi/snowball/mc_cuda/implied_volatility", [=, snowball = snowball, snowball_quote = snowball_quote]() -> Result<double> {
        if (!snowball_quote) return std::unexpected(snowball_quote.error());
        return implied_volatility(cuda_snowball, snowball, context, *snowball_quote); }, "volatility")->UseRealTime();
#endif
    const auto daily_context = *make_pricing_context(base_context.model_parameters(), 100.25, base_context.valuation_time(), all_days_calendar());
    register_price("analytics/kiyosi/accumulator/fd/calendar_daily/price", accumulator,
                   FiniteDifferenceAccumulatorEngine{{.asset_step_count = 80, .time_step_count = 1, .asset_upper_boundary = 400.0}}, daily_context);
    register_price("analytics/kiyosi/accumulator/fd/calendar_daily/split_grid/price", accumulator,
                   FiniteDifferenceAccumulatorEngine{}, daily_context);
    const auto weekday_context = *make_pricing_context(base_context.model_parameters(), 100.25, base_context.valuation_time(), weekdays_calendar());
    register_price("analytics/kiyosi/accumulator/fd/calendar_weekdays/price", accumulator,
                   FiniteDifferenceAccumulatorEngine{{.asset_step_count = 80, .time_step_count = 1, .asset_upper_boundary = 400.0}}, weekday_context);
    register_price("analytics/kiyosi/accumulator/fd/calendar_weekdays/split_grid/price", accumulator,
                   FiniteDifferenceAccumulatorEngine{}, weekday_context);
    const FiniteDifferenceSnowballEngine snowball_engine{};
    const auto barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out});
    const auto knock_in_barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .barrier_level = 80.0, .barrier_type = BarrierType::down_and_in});
    register_price("analytics/kiyosi/barrier/analytic/down_and_in/price", knock_in_barrier, AnalyticBarrierEngine{}, base_context);
    register_price("analytics/kiyosi/barrier/analytic/down_and_out/price", barrier, AnalyticBarrierEngine{}, base_context);
    const FiniteDifferenceBarrierEngine barrier_engine{};
    const Date dense_expiry{std::chrono::year{2035} / 1 / 1};
    std::vector<Date> dense_dates;
    for (auto date = european.effective_date() + std::chrono::days{1}; date <= dense_expiry; date += std::chrono::days{1})
        if (weekdays_calendar().is_trading_day(date)) dense_dates.push_back(date);
    const auto dense_barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = european.effective_date(), .expiry_date = dense_expiry, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = dense_dates, .touch_state = BarrierTouchState::untouched});
    register_calculation("analytics/kiyosi/barrier/analytic/scheduled_dense/all", [=, dense_barrier = dense_barrier] { return AnalyticBarrierEngine{}.price_with_greeks(dense_barrier, context, true); }, "price");
    register_calculation("analytics/kiyosi/barrier/analytic/scheduled_dense/scalar_stencils", [=, dense_barrier = dense_barrier] { return calculate_numerical_greeks(AnalyticBarrierEngine{}, dense_barrier, context); }, "price");
    const auto dense_snowball = *make_snowball_option({.knock_out_coupon_rates = std::vector<double>(dense_dates.size(), 0.08), .maturity_coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = std::vector<double>(dense_dates.size(), 1'000.0), .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = dense_dates, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = european.effective_date(), .expiry_date = dense_expiry});
    const auto dense_context = *make_pricing_context(base_context.model_parameters(), 100.0, start_of_day(dense_expiry) - std::chrono::hours{12}, all_days_calendar());
    const auto dense_barrier_context = *make_pricing_context(base_context.model_parameters(), 100.25, dense_context.valuation_time(), weekdays_calendar());
    const FiniteDifferenceBarrierEngine dense_barrier_engine{80, 1};
    register_price("analytics/kiyosi/barrier/fd/scheduled_dense/price", dense_barrier, dense_barrier_engine, dense_barrier_context);
    register_calculation("analytics/kiyosi/barrier/fd/scheduled_dense/vega_rho", [=, dense_barrier = dense_barrier] { return dense_barrier_engine.price_with_greeks(dense_barrier, dense_barrier_context, {Greek::vega, Greek::rho}); }, "price");
    register_calculation("analytics/kiyosi/barrier/fd/scheduled_dense/scalar_stencils", [=, dense_barrier = dense_barrier] { return detail::price_with_greeks(dense_barrier_engine, dense_barrier, dense_barrier_context, {Greek::vega, Greek::rho}, NumericalShiftSettings{}); }, "price");
    const MonteCarloSnowballEngine dense_monte_carlo{1, 42};
    const FiniteDifferenceSnowballEngine dense_finite_difference{80, 1};
    register_price("analytics/kiyosi/snowball/mc/scheduled_dense/price", dense_snowball, dense_monte_carlo, dense_context);
    register_calculation("analytics/kiyosi/snowball/mc/scheduled_dense/vega_rho", [=, dense_snowball = dense_snowball] { return dense_monte_carlo.price_with_greeks(dense_snowball, dense_context, {Greek::vega, Greek::rho}); }, "price");
    register_price("analytics/kiyosi/snowball/fd/scheduled_dense/price", dense_snowball, dense_finite_difference, dense_context);
    register_calculation("analytics/kiyosi/snowball/fd/scheduled_dense/vega_rho", [=, dense_snowball = dense_snowball] { return dense_finite_difference.price_with_greeks(dense_snowball, dense_context, {Greek::vega, Greek::rho}); }, "price");
    const auto dense_coupon_quote = dense_finite_difference.price(dense_snowball, dense_context);
    register_calculation("analytics/kiyosi/snowball/fd/scheduled_dense/implied_coupon", [=, dense_snowball = dense_snowball, dense_coupon_quote = dense_coupon_quote]() -> Result<double> {
                             if (!dense_coupon_quote) return std::unexpected(dense_coupon_quote.error());
                             return implied_coupon(dense_finite_difference, dense_snowball, dense_context, *dense_coupon_quote,
                                                   CouponQuoteConvention::shift_maturity_coupon); }, "coupon");
    register_calculation("analytics/kiyosi/snowball/fd/scheduled_dense/implied_coupon/independent_trials", [=, dense_snowball = dense_snowball, dense_coupon_quote = dense_coupon_quote]() -> Result<double> {
                             if (!dense_coupon_quote) return std::unexpected(dense_coupon_quote.error());
                             return detail::solve_implied_coupon(dense_finite_difference, dense_snowball, dense_context, *dense_coupon_quote, ImpliedCouponSettings{},
                                                                 [](const auto& note, double coupon) { return detail::replace_coupon(note, coupon, CouponQuoteConvention::shift_maturity_coupon); }); }, "coupon");
    const auto upper_barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out});
    register_price("analytics/kiyosi/barrier/fd/continuous_up_and_out/price", upper_barrier, barrier_engine, base_context);
    const FiniteDifferenceVanillaEngine vanilla_engine{};
    const MonteCarloVanillaEngine monte_carlo_engine{100'000, 50, 42};
    const auto american_put = *make_american_option(OptionType::put, 100.0, european.effective_date(), european.expiry_date());
    register_price("analytics/kiyosi/american/mc/price", american_put, monte_carlo_engine, context)->UseRealTime();
    const double american_quote = *monte_carlo_engine.price(american_put, context);
    register_calculation("analytics/kiyosi/american/mc/implied_volatility", [=] { return implied_volatility(monte_carlo_engine, american_put, context, american_quote); }, "volatility")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/delta_gamma", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/unavailable_delta", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::delta}, {.spot_shift = 200.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/unavailable_speed", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/unavailable_vanna", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::vanna}, {.volatility_shift = 0.3}); }, "price")->UseRealTime();
#if KIYOSI_HAS_CUDA
    const MonteCarloVanillaEngine cuda_american{100'000, 50, 42, MonteCarloBackend::cuda};
    register_price("analytics/kiyosi/european/mc_cuda/price", european, cuda_american, context)->UseRealTime();
    register_calculation("analytics/kiyosi/european/mc_cuda/unavailable_delta", [=] { return cuda_american.price_with_greeks(european, context, {Greek::delta}, {.spot_shift = 200.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/european/mc_cuda/unavailable_speed", [=] { return cuda_american.price_with_greeks(european, context, {Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/european/mc_cuda/unavailable_vanna", [=] { return cuda_american.price_with_greeks(european, context, {Greek::vanna}, {.volatility_shift = 0.3}); }, "price")->UseRealTime();
    register_price("analytics/kiyosi/american/mc_cuda/price", american_put, cuda_american, context)->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc_cuda/delta_gamma", [=] { return cuda_american.price_with_greeks(american_put, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc_cuda/speed", [=] { return cuda_american.price_with_greeks(american_put, context, {Greek::speed}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc_cuda/all", [=] { return cuda_american.price_with_greeks(american_put, context, true); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc_cuda/unavailable_delta", [=] { return cuda_american.price_with_greeks(american_put, context, {Greek::delta}, {.spot_shift = 200.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc_cuda/unavailable_speed", [=] { return cuda_american.price_with_greeks(american_put, context, {Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
#endif
    register_calculation("analytics/kiyosi/european/mc/delta_gamma", [=] { return monte_carlo_engine.price_with_greeks(european, context, {Greek::delta, Greek::gamma}); }, "price");
    register_calculation("analytics/kiyosi/european/mc/unavailable_delta", [=] { return monte_carlo_engine.price_with_greeks(european, context, {Greek::delta}, {.spot_shift = 200.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/european/mc/unavailable_speed", [=] { return monte_carlo_engine.price_with_greeks(european, context, {Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/european/mc/unavailable_vanna", [=] { return monte_carlo_engine.price_with_greeks(european, context, {Greek::vanna}, {.volatility_shift = 0.3}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/european/mc/all", [=] { return monte_carlo_engine.price_with_greeks(european, context, true); }, "price");
    register_calculation("analytics/kiyosi/european/mc/numerical_all", [=] { return calculate_numerical_greeks(monte_carlo_engine, european, context); }, "price");
    register_price("analytics/kiyosi/european/fd/exact_node/fine_space/price", european,
                   FiniteDifferenceVanillaEngine{{.asset_step_count = 10'000, .time_step_count = 1, .asset_upper_boundary = 400.0}}, base_context);
    const auto off_node_context = *make_pricing_context(base_context.model_parameters(), 100.25, base_context.valuation_time(), base_context.calendar());
    const FiniteDifferenceVanillaEngine fine_space_engine{{.asset_step_count = 10'000, .time_step_count = 1, .asset_upper_boundary = 400.0}};
    register_calculation("analytics/kiyosi/european/fd/off_node/fine_space/speed", [=] { return fine_space_engine.price_with_greeks(european, off_node_context, {Greek::speed}); }, "price");
    const CoxRossRubinsteinVanillaEngine binomial_engine{1'000};
    register_calculation("analytics/kiyosi/european/crr/speed", [=] { return binomial_engine.price_with_greeks(european, context, {Greek::speed}); }, "price");
    register_calculation("analytics/kiyosi/european/fd/speed", [=] { return vanilla_engine.price_with_greeks(european, context, {Greek::speed}); }, "price");
    const auto digital = *make_cash_or_nothing_option(OptionType::call, 100.0, 10.0, european.effective_date(), european.expiry_date());
    register_calculation("analytics/kiyosi/cash_digital/fd/speed", [=] { return FiniteDifferenceDigitalEngine{}.price_with_greeks(digital, context, {Greek::speed}); }, "price");
    register_calculation("analytics/kiyosi/barrier/fd/speed", [=, barrier = barrier] { return barrier_engine.price_with_greeks(barrier, base_context, {Greek::speed}); }, "price");
    register_calculation("analytics/kiyosi/accumulator/fd/speed", [=] { return accumulator_engine.price_with_greeks(accumulator, context, {Greek::speed}); }, "price");
    register_calculation("analytics/kiyosi/snowball/fd/speed", [=, snowball = snowball] { return snowball_engine.price_with_greeks(snowball, context, {Greek::speed}); }, "price");
    register_calculation("analytics/kiyosi/european/crr/vanna", [=] { return binomial_engine.price_with_greeks(european, context, {Greek::vanna}); }, "price");
    register_calculation("analytics/kiyosi/european/crr/zomma", [=] { return binomial_engine.price_with_greeks(european, context, {Greek::zomma}); }, "price");
    register_calculation("analytics/kiyosi/european/crr/charm", [=] { return binomial_engine.price_with_greeks(european, context, {Greek::charm}); }, "price");
    register_calculation("analytics/kiyosi/european/crr/color", [=] { return binomial_engine.price_with_greeks(european, context, {Greek::color}); }, "price");
    register_calculation("analytics/kiyosi/european/fd/vanna", [=] { return vanilla_engine.price_with_greeks(european, base_context, {Greek::vanna}); }, "price");
    register_price("analytics/kiyosi/european/fd/price", european, vanilla_engine, base_context);
    register_calculation("analytics/kiyosi/european/fd/zomma", [=] { return vanilla_engine.price_with_greeks(european, base_context, {Greek::zomma}); }, "price");
    register_calculation("analytics/kiyosi/european/fd/charm", [=] { return vanilla_engine.price_with_greeks(european, context, {Greek::charm}); }, "price");
    register_calculation("analytics/kiyosi/european/fd/color", [=] { return vanilla_engine.price_with_greeks(european, context, {Greek::color}); }, "price");
    register_calculation("analytics/kiyosi/european/crr/effective_date/charm", [=] { return binomial_engine.price_with_greeks(european, base_context, {Greek::charm}); }, "price");
    register_calculation("analytics/kiyosi/european/crr/effective_date/color", [=] { return binomial_engine.price_with_greeks(european, base_context, {Greek::color}); }, "price");
    register_calculation("analytics/kiyosi/european/fd/effective_date/charm", [=] { return vanilla_engine.price_with_greeks(european, base_context, {Greek::charm}); }, "price");
    register_calculation("analytics/kiyosi/european/fd/effective_date/color", [=] { return vanilla_engine.price_with_greeks(european, base_context, {Greek::color}); }, "price");
    register_price("analytics/kiyosi/barrier/fd/price", barrier, barrier_engine, base_context);
    register_calculation("analytics/kiyosi/barrier/fd/delta_gamma", [=, barrier = barrier] { return barrier_engine.price_with_greeks(barrier, base_context, {Greek::delta, Greek::gamma}); }, "price");
    const auto scheduled_knock_in = *make_barrier_option(
        {.option_type = OptionType::call, .strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .barrier_level = 80.0, .barrier_type = BarrierType::down_and_in, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = make_monthly_schedule(european.effective_date(), european.expiry_date(), 1, base_context.calendar())->dates()});
    register_price("analytics/kiyosi/barrier/fd/scheduled_knock_in/price", scheduled_knock_in, barrier_engine, base_context);
    register_price("analytics/kiyosi/accumulator/fd/price", accumulator, accumulator_engine, context);
    const Date leap_start{std::chrono::year{2024} / 1 / 1}, leap_end{std::chrono::year{2025} / 1 / 1};
    const auto leap_accumulator = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 0.0, .effective_date = leap_start, .expiry_date = leap_end});
    const auto leap_context = *make_pricing_context(base_context.model_parameters(), 100.0, leap_start, all_days_calendar());
    register_price("analytics/kiyosi/accumulator/fd/leap_year/daily_grid/price", leap_accumulator,
                   FiniteDifferenceAccumulatorEngine{{200, 366}}, leap_context);
    const auto fixed_accumulator = *make_accumulator(
        {.strike = 50.0, .knock_out_level = 200.0, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 3.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/fixed_quantity/price", fixed_accumulator, accumulator_engine, context);
    const auto zero_carry_context = *make_pricing_context(
        *make_bsm_parameters(0.0, 0.0, 0.2), context.spot_price(), context.valuation_time(), context.calendar());
    register_price("analytics/kiyosi/accumulator/fd/zero_carry/price", fixed_accumulator, accumulator_engine, zero_carry_context);
    register_price("analytics/kiyosi/accumulator/mc/zero_carry/price", fixed_accumulator, MonteCarloAccumulatorEngine{20'000, 42}, zero_carry_context);
    const auto empty_accumulator = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 0.0, .acceleration_factor = 2.0, .accumulated_quantity = 0.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/zero_exposure/price", empty_accumulator, accumulator_engine, context);
    register_price("analytics/kiyosi/accumulator/mc/zero_exposure/price", empty_accumulator, MonteCarloAccumulatorEngine{2'000, 42}, context);
    const auto inactive_accumulator = *make_accumulator(
        {.strike = 110.0, .knock_out_level = 105.0, .daily_quantity = 1.0, .acceleration_factor = 0.0, .accumulated_quantity = 0.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/zero_acceleration/price", inactive_accumulator, accumulator_engine, context);
    register_price("analytics/kiyosi/accumulator/mc/zero_acceleration/price", inactive_accumulator, MonteCarloAccumulatorEngine{20'000, 42}, context);
    const auto inactive_fixed_accumulator = *make_accumulator(
        {.strike = 110.0, .knock_out_level = 105.0, .daily_quantity = 1.0, .acceleration_factor = 0.0, .accumulated_quantity = 3.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/inactive_accrual/price", inactive_fixed_accumulator, accumulator_engine, context);
    register_calculation("analytics/kiyosi/accumulator/fd/delta_gamma", [=] { return accumulator_engine.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma}); }, "price");
    register_price("analytics/kiyosi/snowball/fd/price", snowball, snowball_engine, context);
    const auto coupon_quote = snowball_engine.price(snowball, context);
    register_calculation("analytics/kiyosi/snowball/fd/implied_coupon", [=, snowball = snowball, coupon_quote = coupon_quote]() -> Result<double> {
                             if (!coupon_quote) return std::unexpected(coupon_quote.error());
                             return implied_coupon(snowball_engine, snowball, context, *coupon_quote,
                                                   CouponQuoteConvention::shift_maturity_coupon); }, "coupon");
    register_calculation("analytics/kiyosi/snowball/fd/implied_coupon/independent_trials", [=, snowball = snowball, coupon_quote = coupon_quote]() -> Result<double> {
                             if (!coupon_quote) return std::unexpected(coupon_quote.error());
                             return detail::solve_implied_coupon(snowball_engine, snowball, context, *coupon_quote, ImpliedCouponSettings{},
                                                                 [](const auto& note, double coupon) { return detail::replace_coupon(note, coupon, CouponQuoteConvention::shift_maturity_coupon); }); }, "coupon");
    const MonteCarloSnowballEngine coupon_engine{20'000, 42};
    const auto monte_carlo_quote = coupon_engine.price(snowball, context);
    register_calculation("analytics/kiyosi/snowball/mc/implied_coupon", [=, snowball = snowball, monte_carlo_quote = monte_carlo_quote]() -> Result<double> {
                             if (!monte_carlo_quote) return std::unexpected(monte_carlo_quote.error());
                             return implied_coupon(coupon_engine, snowball, context, *monte_carlo_quote,
                                                   CouponQuoteConvention::shift_maturity_coupon); }, "coupon");
    register_calculation("analytics/kiyosi/snowball/fd/delta_gamma", [=, snowball = snowball] { return snowball_engine.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price");
    const auto knocked_in = snowball_scenario(AutocallableBarrierState::knocked_in).first;
    register_price("analytics/kiyosi/snowball/fd/knocked_in/price", knocked_in, snowball_engine, context);
    const auto expiry_note = *make_snowball_option(
        {.knock_out_coupon_rates = {0.08}, .maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {110.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {european.expiry_date()}, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/snowball/fd/expiry_only/price", expiry_note, snowball_engine, base_context);
    register_price("analytics/kiyosi/snowball/fd/expiry_only/fine_time_grid/price", expiry_note, FiniteDifferenceSnowballEngine{{200, 1000}}, base_context);
    const auto fixed_note = *make_snowball_option(
        {.knock_out_coupon_rates = {0.0}, .maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {110.0}, .upper_strike = 100.0, .lower_strike = 100.0, .observation_dates = {european.expiry_date()}, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    const auto fixed_context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, european.effective_date());
    register_price("analytics/kiyosi/snowball/mc/fixed/price", fixed_note, MonteCarloSnowballEngine{20'000, 42}, fixed_context);
    register_price("analytics/kiyosi/snowball/fd/fixed/price", fixed_note, snowball_engine, fixed_context);
    const auto protected_note = *make_snowball_option(
        {.knock_out_coupon_rates = {0.08, 0.08}, .maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {110.0, 104.0}, .upper_strike = 100.0, .lower_strike = 100.0, .observation_dates = {Date{std::chrono::year{2025} / 7 / 1}, european.expiry_date()}, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/snowball/mc/protected/price", protected_note, MonteCarloSnowballEngine{20'000, 42}, base_context);
    register_price("analytics/kiyosi/snowball/fd/protected/price", protected_note, snowball_engine, base_context);
    const MonteCarloVanillaEngine monte_carlo{{5'000, 10, 42}};
    const auto no_dividend = *make_pricing_context(*make_bsm_parameters(0.05, 0.0, 0.2), 100.0, european.effective_date());
    const auto american_call = *make_american_option(OptionType::call, 100.0, european.effective_date(), european.expiry_date());
    register_price("analytics/kiyosi/american/mc/no_dividend/price", american_call, MonteCarloVanillaEngine{100'000, 50, 42}, no_dividend);
    const auto fd_quote = vanilla_engine.price(european, context);
    register_calculation("analytics/kiyosi/european/fd/implied_volatility", [=, fd_quote = fd_quote]() -> Result<double> {
                             if (!fd_quote) return std::unexpected(fd_quote.error());
                             return implied_volatility(vanilla_engine, european, context, *fd_quote); }, "volatility");
    const auto quote = monte_carlo.price(european, context);
    register_calculation("analytics/kiyosi/european/mc/implied_volatility", [=, quote = quote]() -> Result<double> {
                             if (!quote) return std::unexpected(quote.error());
                             return implied_volatility(monte_carlo, european, context, *quote); }, "volatility");
}

void kiyosi::benchmark_support::register_monte_carlo_cases()
{
    using namespace kiyosi;
    for (const auto backend : {MonteCarloBackend::cpu
#if KIYOSI_HAS_CUDA
                               ,
                               MonteCarloBackend::cuda
#endif
         }) {
        const std::string suffix = backend == MonteCarloBackend::cpu ? "_cpu" : "_cuda";
        const MonteCarloVanillaEngine vanilla{{1'000'000, 50, 42, backend}};
        register_monte_carlo("bm_monte_carlo_european" + suffix, monte_carlo_scenario(), vanilla);
        register_monte_carlo("bm_monte_carlo_american" + suffix, american_monte_carlo_scenario(), vanilla);
        register_monte_carlo("bm_monte_carlo_accumulator" + suffix, accumulator_scenario(),
                             MonteCarloAccumulatorEngine{{250'000, 42, backend}});
        register_monte_carlo("bm_monte_carlo_phoenix" + suffix, phoenix_scenario(),
                             MonteCarloPhoenixEngine{{250'000, 42, backend}});
        register_monte_carlo("bm_monte_carlo_snowball" + suffix, snowball_scenario(),
                             MonteCarloSnowballEngine{{250'000, 42, backend}});
    }
}
