#include <chrono>
#include <array>
#include <format>
#include <initializer_list>
#include <string>
#include <utility>
#include <vector>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>
#include "pricing/detail/numerical_greeks.hpp"
#include "pricing/detail/fd_grid.hpp"
#include "pricing/engines/monte_carlo_parallel.hpp"

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

auto american_monte_carlo_scenario(kiyosi::OptionType option_type = kiyosi::OptionType::put)
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective_date);
    return std::pair{
        *kiyosi::make_american_option(
            option_type, 100.0, effective_date, expiry_date),
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
        *kiyosi::make_phoenix_option({.coupon_rate = 0.002, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 108.0, 106.0, 104.0}, std::nullopt, std::vector<double>{90.0, 90.0, 90.0, 90.0})}),
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
        *kiyosi::make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = barrier_state, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 108.0, 106.0, 104.0}, std::vector<double>{0.08, 0.08, 0.08, 0.08})}),
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
    const Date range_start{std::chrono::year{2025} / 1 / 1};
    const auto range = make_range_accrual_option({1'000, 0.12, 95, 110, range_start, range_start + std::chrono::days{30}, {}}).value();
    const auto range_context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100, range_start, weekdays_calendar()).value();
    const AnalyticRangeAccrualEngine range_engine;
    register_price("range_accrual/analytic/price", range, range_engine, range_context)->UseRealTime();
    register_calculation("range_accrual/analytic/greeks", [=] { return range_engine.price_with_greeks(range, range_context, {Greek::delta, Greek::gamma, Greek::rho}); }, "price")->UseRealTime();
    for (const bool together : {false, true})
        benchmark::RegisterBenchmark(together ? "analytics/kiyosi/fd/nonuniform/sample" : "analytics/kiyosi/fd/nonuniform/separate_samples", [together](benchmark::State& state) {
            detail::SpatialGrid grid{800.5, 1.0, 800};
            grid.nodes.resize(grid.size());
            for (std::size_t index = 0; index < grid.nodes.size(); ++index)
                grid.nodes[index] = static_cast<double>(index);
            grid.nodes.back() = grid.upper;
            std::vector<double> values(grid.size(), 1.0), curvature;
            values.front() = 0.0;
            benchmark::DoNotOptimize(grid.sample(values, 1.5, curvature));
            for (auto _ : state) {
                auto sample = together ? grid.sample(values, 1.5, curvature)
                                       : std::array{grid.interpolate(values, 1.5, curvature), grid.delta(values, 1.5, curvature), grid.gamma(values, 1.5, curvature)};
                benchmark::DoNotOptimize(sample);
            }
        })->UseRealTime();
    const auto [european, base_context] = monte_carlo_scenario();
    for (const int count : {0, 1000, 10'000}) {
        std::vector<Date> closed;
        closed.reserve(static_cast<std::size_t>(count));
        for (int index = 0; index < count; ++index)
            closed.push_back(european.effective_date() + std::chrono::days{index + 500});
        const auto calendar = make_custom_calendar(weekdays_calendar(), std::move(closed)).value();
        const auto market = make_pricing_context(base_context.model_parameters(), 100.0, base_context.valuation_time(), calendar).value();
        register_calculation(std::format("analytics/kiyosi/context/custom_calendar/{}/shift", count).c_str(), [market]() -> Result<double> {
            auto shifted = detail::shifted_context(market, 101.0, market.model_parameters().volatility(), (*market.model_parameters().risk_free_rate()), market.valuation_time());
            if (!shifted) return std::unexpected(shifted.error());
            benchmark::DoNotOptimize(*shifted);
            return shifted->spot_price(); }, "spot")->UseRealTime();
    }
    register_calculation("analytics/kiyosi/european/analytic/delta", [=] { return AnalyticVanillaEngine{}.price_with_greeks(european, base_context, {Greek::delta}); }, "price");
    register_calculation("analytics/kiyosi/european/analytic/rho", [=] { return AnalyticVanillaEngine{}.price_with_greeks(european, base_context, {Greek::rho}); }, "price");
    const auto context = *make_pricing_context(
        base_context.model_parameters(), base_context.spot_price(),
        base_context.valuation_time() + std::chrono::hours{12}, base_context.calendar());
    const auto accumulator = accumulator_scenario().first;
    const auto snowball = snowball_scenario().first;
    const auto snowball_monte_carlo = make_monte_carlo_autocallable_engine({20'000, 42}).value();
    register_price("analytics/kiyosi/snowball/mc/price", snowball, snowball_monte_carlo, context)->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/delta_gamma", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/vega_vanna_zomma", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, {Greek::vega, Greek::vanna, Greek::zomma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/all", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, true); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/delta_gamma_rho", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma, Greek::rho}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/mc/delta_gamma_unavailable_speed", [=, snowball = snowball] { return snowball_monte_carlo.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma, Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    const FiniteDifferenceAccumulatorEngine accumulator_engine{};
    const auto accumulator_monte_carlo = make_monte_carlo_accumulator_engine({20'000, 42}).value();
    register_price("analytics/kiyosi/accumulator/mc/price", accumulator, accumulator_monte_carlo, context)->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/delta_gamma", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/vega_vanna_zomma", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::vega, Greek::vanna, Greek::zomma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/delta_gamma_unavailable_speed", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma, Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/rho", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::rho}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/delta_gamma_rho", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma, Greek::rho}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/mc/all", [=] { return accumulator_monte_carlo.price_with_greeks(accumulator, context, true); }, "price")->UseRealTime();
#if KIYOSI_HAS_CUDA
    const auto cuda_accumulator = make_monte_carlo_accumulator_engine({250'000, 42, MonteCarloBackend::cuda}).value();
    const auto cuda_snowball = make_monte_carlo_autocallable_engine({250'000, 42, MonteCarloBackend::cuda}).value();
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
                   make_finite_difference_accumulator_engine({.asset_step_count = 80, .time_step_count = 1, .asset_upper_boundary = 400.0}).value(), daily_context);
    register_price("analytics/kiyosi/accumulator/fd/calendar_daily/split_grid/price", accumulator,
                   FiniteDifferenceAccumulatorEngine{}, daily_context);
    const auto weekday_context = *make_pricing_context(base_context.model_parameters(), 100.25, base_context.valuation_time(), weekdays_calendar());
    register_price("analytics/kiyosi/accumulator/fd/calendar_weekdays/price", accumulator,
                   make_finite_difference_accumulator_engine({.asset_step_count = 80, .time_step_count = 1, .asset_upper_boundary = 400.0}).value(), weekday_context);
    register_price("analytics/kiyosi/accumulator/fd/calendar_weekdays/split_grid/price", accumulator,
                   FiniteDifferenceAccumulatorEngine{}, weekday_context);
    const FiniteDifferenceAutocallableEngine snowball_engine{};
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
    register_calculation("analytics/kiyosi/barrier/analytic/scheduled_dense/scalar_stencils", [=, dense_barrier = dense_barrier] { return detail::price_with_greeks(AnalyticBarrierEngine{}, dense_barrier, context, true, NumericalShiftSettings{}); }, "price");
    const auto dense_snowball = *make_snowball_option({.maturity_coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = european.effective_date(), .expiry_date = dense_expiry, .observations = *kiyosi::make_autocallable_observations(dense_dates, std::vector<double>(dense_dates.size(), 1'000.0), std::vector<double>(dense_dates.size(), 0.08))});
    const auto dense_context = *make_pricing_context(base_context.model_parameters(), 100.0, start_of_day(dense_expiry) - std::chrono::hours{12}, all_days_calendar());
    const auto dense_barrier_context = *make_pricing_context(base_context.model_parameters(), 100.25, dense_context.valuation_time(), weekdays_calendar());
    register_calculation("analytics/kiyosi/barrier/scheduled_dense/time_stencil", [=, dense_barrier = dense_barrier] { return detail::time_stencil_available(dense_barrier, dense_barrier_context, dense_context.valuation_time() - std::chrono::hours{1}, dense_context.valuation_time() + std::chrono::hours{1}); }, "available")->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/scheduled_dense/time_stencil", [=, dense_snowball = dense_snowball] { return detail::time_stencil_available(dense_snowball, dense_context, dense_context.valuation_time() - std::chrono::hours{1}, dense_context.valuation_time() + std::chrono::hours{1}); }, "available")->UseRealTime();
    const auto dense_barrier_engine = make_finite_difference_barrier_engine({80, 1}).value();
    register_price("analytics/kiyosi/barrier/fd/scheduled_dense/price", dense_barrier, dense_barrier_engine, dense_barrier_context);
    register_calculation("analytics/kiyosi/barrier/fd/scheduled_dense/vega_rho", [=, dense_barrier = dense_barrier] { return dense_barrier_engine.price_with_greeks(dense_barrier, dense_barrier_context, {Greek::vega, Greek::rho}); }, "price");
    register_calculation("analytics/kiyosi/barrier/fd/scheduled_dense/scalar_stencils", [=, dense_barrier = dense_barrier] { return detail::price_with_greeks(dense_barrier_engine, dense_barrier, dense_barrier_context, {Greek::vega, Greek::rho}, NumericalShiftSettings{}); }, "price");
    const auto dense_monte_carlo = make_monte_carlo_autocallable_engine({1, 42}).value();
    const auto dense_finite_difference = make_finite_difference_autocallable_engine({80, 1}).value();
    register_price("analytics/kiyosi/snowball/mc/scheduled_dense/price", dense_snowball, dense_monte_carlo, dense_context);
    register_calculation("analytics/kiyosi/snowball/mc/scheduled_dense/vega_rho", [=, dense_snowball = dense_snowball] { return dense_monte_carlo.price_with_greeks(dense_snowball, dense_context, {Greek::vega, Greek::rho}); }, "price");
    register_price("analytics/kiyosi/snowball/fd/scheduled_dense/price", dense_snowball, dense_finite_difference, dense_context);
    register_calculation("analytics/kiyosi/snowball/fd/scheduled_dense/vega_rho", [=, dense_snowball = dense_snowball] { return dense_finite_difference.price_with_greeks(dense_snowball, dense_context, {Greek::vega, Greek::rho}); }, "price");
    const auto dense_quadrature = make_quadrature_autocallable_engine({16, 8.0}).value();
    register_price("analytics/kiyosi/snowball/quadrature/scheduled_dense/price", dense_snowball, dense_quadrature, dense_context)->UseRealTime();
    register_calculation("analytics/kiyosi/snowball/quadrature/scheduled_dense/delta_gamma", [=, dense_snowball = dense_snowball] { return dense_quadrature.price_with_greeks(dense_snowball, dense_context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
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
    const auto monte_carlo_engine = make_monte_carlo_vanilla_engine({100'000, 50, 42}).value();
    const auto american_put = *make_american_option(OptionType::put, 100.0, european.effective_date(), european.expiry_date());
    register_price("analytics/kiyosi/american/mc/price", american_put, monte_carlo_engine, context)->UseRealTime();
    const double american_quote = *monte_carlo_engine.price(american_put, context);
    register_calculation("analytics/kiyosi/american/mc/implied_volatility", [=] { return implied_volatility(monte_carlo_engine, american_put, context, american_quote); }, "volatility")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/delta_gamma", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/unavailable_delta", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::delta}, {.spot_shift = 200.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/unavailable_speed", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::speed}, {.spot_shift = 60.0}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/american/mc/unavailable_vanna", [=] { return monte_carlo_engine.price_with_greeks(american_put, context, {Greek::vanna}, {.volatility_shift = 0.3}); }, "price")->UseRealTime();
#if KIYOSI_HAS_CUDA
    const auto cuda_american = make_monte_carlo_vanilla_engine({100'000, 50, 42, MonteCarloBackend::cuda}).value();
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
    register_price("analytics/kiyosi/european/fd/exact_node/fine_space/price", european,
                   make_finite_difference_vanilla_engine({.asset_step_count = 10'000, .time_step_count = 1, .asset_upper_boundary = 400.0}).value(), base_context);
    const auto off_node_context = *make_pricing_context(base_context.model_parameters(), 100.25, base_context.valuation_time(), base_context.calendar());
    const auto fine_space_engine = make_finite_difference_vanilla_engine({.asset_step_count = 10'000, .time_step_count = 1, .asset_upper_boundary = 400.0}).value();
    register_calculation("analytics/kiyosi/european/fd/off_node/fine_space/speed", [=] { return fine_space_engine.price_with_greeks(european, off_node_context, {Greek::speed}); }, "price");
    const auto binomial_engine = make_cox_ross_rubinstein_vanilla_engine({1'000}).value();
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
    for (const bool daily : {false, true}) {
        auto dates = scheduled_knock_in.observation_dates();
        if (daily) {
            dates.clear();
            for (auto date = european.effective_date() + std::chrono::days{1}; date <= european.expiry_date(); date += std::chrono::days{1})
                if (base_context.calendar().is_trading_day(date)) dates.push_back(date);
        }
        for (const bool knock_in : {false, true}) {
            const auto monitored = *make_barrier_option(
                {.option_type = OptionType::call, .strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .barrier_level = 110.0, .barrier_type = knock_in ? BarrierType::up_and_in : BarrierType::up_and_out, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = dates});
            const auto name = std::format("analytics/kiyosi/barrier/fd/observations_{}/{}/price", daily ? "daily" : "monthly", knock_in ? "knock_in" : "knock_out");
            register_price(name.c_str(), monitored, barrier_engine, base_context);
        }
    }
    register_price("analytics/kiyosi/accumulator/fd/price", accumulator, accumulator_engine, context);
    register_calculation("analytics/kiyosi/accumulator/fd/vega_rho", [=] { return accumulator_engine.price_with_greeks(accumulator, context, {Greek::vega, Greek::rho}); }, "price")->UseRealTime();
    register_calculation("analytics/kiyosi/accumulator/fd/vega_rho/independent_stencils", [=] { return detail::price_with_greeks(accumulator_engine, accumulator, context, {Greek::vega, Greek::rho}, NumericalShiftSettings{}); }, "price")->UseRealTime();
    const auto accumulator_fd_quote = accumulator_engine.price(accumulator, context);
    for (const bool independent : {false, true})
        register_calculation(independent ? "analytics/kiyosi/accumulator/fd/implied_volatility/independent_trials" : "analytics/kiyosi/accumulator/fd/implied_volatility", [=, accumulator_fd_quote = accumulator_fd_quote]() -> Result<double> {
            if (!accumulator_fd_quote) return std::unexpected(accumulator_fd_quote.error());
            const ImpliedVolatilitySettings bounds{.lower_bound = 0.15, .upper_bound = 0.25};
            return independent ? detail::solve_implied_volatility(accumulator_engine, accumulator, context, *accumulator_fd_quote, bounds,
                                                                  [](const auto& engine, const auto& instrument, const auto& market) { return engine.price(instrument, market); })
                               : implied_volatility(accumulator_engine, accumulator, context, *accumulator_fd_quote, bounds); }, "volatility")->UseRealTime();
    const Date leap_start{std::chrono::year{2024} / 1 / 1}, leap_end{std::chrono::year{2025} / 1 / 1};
    const auto leap_accumulator = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .accumulated_quantity = 0.0, .effective_date = leap_start, .expiry_date = leap_end});
    const auto leap_context = *make_pricing_context(base_context.model_parameters(), 100.0, leap_start, all_days_calendar());
    register_price("analytics/kiyosi/accumulator/fd/leap_year/daily_grid/price", leap_accumulator,
                   make_finite_difference_accumulator_engine({200, 366}).value(), leap_context);
    const auto fixed_accumulator = *make_accumulator(
        {.strike = 50.0, .knock_out_level = 200.0, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 3.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/fixed_quantity/price", fixed_accumulator, accumulator_engine, context);
    const auto zero_carry_context = *make_pricing_context(
        *make_bsm_parameters(0.0, 0.0, 0.2), context.spot_price(), context.valuation_time(), context.calendar());
    register_price("analytics/kiyosi/accumulator/fd/zero_carry/price", fixed_accumulator, accumulator_engine, zero_carry_context);
    register_price("analytics/kiyosi/accumulator/mc/zero_carry/price", fixed_accumulator, make_monte_carlo_accumulator_engine({20'000, 42}).value(), zero_carry_context);
    const auto empty_accumulator = *make_accumulator(
        {.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 0.0, .acceleration_factor = 2.0, .accumulated_quantity = 0.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/zero_exposure/price", empty_accumulator, accumulator_engine, context);
    register_price("analytics/kiyosi/accumulator/mc/zero_exposure/price", empty_accumulator, make_monte_carlo_accumulator_engine({2'000, 42}).value(), context);
    const auto inactive_accumulator = *make_accumulator(
        {.strike = 110.0, .knock_out_level = 105.0, .daily_quantity = 1.0, .acceleration_factor = 0.0, .accumulated_quantity = 0.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date()});
    register_price("analytics/kiyosi/accumulator/fd/zero_acceleration/price", inactive_accumulator, accumulator_engine, context);
    register_price("analytics/kiyosi/accumulator/mc/zero_acceleration/price", inactive_accumulator, make_monte_carlo_accumulator_engine({20'000, 42}).value(), context);
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
    const auto coupon_engine = make_monte_carlo_autocallable_engine({20'000, 42}).value();
    const auto monte_carlo_quote = coupon_engine.price(snowball, context);
    register_calculation("analytics/kiyosi/snowball/mc/implied_coupon", [=, snowball = snowball, monte_carlo_quote = monte_carlo_quote]() -> Result<double> {
                             if (!monte_carlo_quote) return std::unexpected(monte_carlo_quote.error());
                             return implied_coupon(coupon_engine, snowball, context, *monte_carlo_quote,
                                                   CouponQuoteConvention::shift_maturity_coupon); }, "coupon");
    register_calculation("analytics/kiyosi/snowball/fd/delta_gamma", [=, snowball = snowball] { return snowball_engine.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price");
    const auto knocked_in = snowball_scenario(AutocallableBarrierState::knocked_in).first;
    register_price("analytics/kiyosi/snowball/fd/knocked_in/price", knocked_in, snowball_engine, context);
    const auto expiry_note = *make_snowball_option(
        {.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .observations = {{european.expiry_date(), 110.0, 0.08}}});
    register_price("analytics/kiyosi/snowball/fd/expiry_only/price", expiry_note, snowball_engine, base_context);
    const QuadratureAutocallableEngine quadrature;
    register_price("analytics/kiyosi/snowball/quadrature/price", snowball, quadrature, context);
    register_price("analytics/kiyosi/snowball/quadrature/knocked_in/price", knocked_in, quadrature, context);
    register_price("analytics/kiyosi/snowball/quadrature/expiry_only/price", expiry_note, quadrature, base_context);
    register_calculation("analytics/kiyosi/snowball/quadrature/delta_gamma", [=, snowball = snowball] { return quadrature.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price");
    register_calculation("analytics/kiyosi/snowball/quadrature/speed", [=, snowball = snowball] { return quadrature.price_with_greeks(snowball, context, {Greek::speed}); }, "price");
    register_calculation("analytics/kiyosi/snowball/quadrature/all_greeks", [=, snowball = snowball] { return quadrature.price_with_greeks(snowball, context, GreeksRequest{true}); }, "price");
    const double quadrature_quote = quadrature.price(snowball, context).value();
    register_calculation("analytics/kiyosi/snowball/quadrature/implied_volatility", [=, snowball = snowball] { return implied_volatility(quadrature, snowball, context, quadrature_quote, {.lower_bound = 0.1, .upper_bound = 0.4}); }, "volatility");
    register_calculation("analytics/kiyosi/snowball/quadrature/implied_coupon", [=, snowball = snowball] { return implied_coupon(quadrature, snowball, context, quadrature_quote, CouponQuoteConvention::shift_maturity_coupon); }, "coupon");
    register_calculation("analytics/kiyosi/snowball/quadrature/implied_coupon/independent_trials", [=, snowball = snowball] { return detail::solve_implied_coupon(quadrature, snowball, context, quadrature_quote, ImpliedCouponSettings{},
                                                                                                                                                                  [](const auto& note, double coupon) { return detail::replace_coupon(note, coupon, CouponQuoteConvention::shift_maturity_coupon); }); }, "coupon");
    register_price("analytics/kiyosi/snowball/fd/expiry_only/fine_time_grid/price", expiry_note, make_finite_difference_autocallable_engine({200, 1000}).value(), base_context);
    const auto fixed_note = *make_snowball_option(
        {.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .observations = {{european.expiry_date(), 110.0, 0.0}}});
    const auto fixed_context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, european.effective_date());
    register_price("analytics/kiyosi/snowball/mc/fixed/price", fixed_note, make_monte_carlo_autocallable_engine({20'000, 42}).value(), fixed_context);
    register_price("analytics/kiyosi/snowball/fd/fixed/price", fixed_note, snowball_engine, fixed_context);
    const auto protected_note = *make_snowball_option(
        {.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 100.0, .effective_date = european.effective_date(), .expiry_date = european.expiry_date(), .observations = {{Date{std::chrono::year{2025} / 7 / 1}, 110.0, 0.08}, {european.expiry_date(), 104.0, 0.08}}});
    register_price("analytics/kiyosi/snowball/mc/protected/price", protected_note, make_monte_carlo_autocallable_engine({20'000, 42}).value(), base_context);
    register_price("analytics/kiyosi/snowball/fd/protected/price", protected_note, snowball_engine, base_context);
    const auto monte_carlo = make_monte_carlo_vanilla_engine({5'000, 10, 42}).value();
    const auto no_dividend = *make_pricing_context(*make_bsm_parameters(0.05, 0.0, 0.2), 100.0, european.effective_date());
    const auto american_call = *make_american_option(OptionType::call, 100.0, european.effective_date(), european.expiry_date());
    register_price("analytics/kiyosi/american/mc/no_dividend/price", american_call, make_monte_carlo_vanilla_engine({100'000, 50, 42}).value(), no_dividend);
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
    for (const bool reuse : {false, true})
        benchmark::RegisterBenchmark(reuse ? "american_lsm/scheduling/reused" : "american_lsm/scheduling/one_shot", [reuse](benchmark::State& state) {
            detail::MonteCarloBlocks blocks{4096, 4};
            std::array<int, 16> partials{};
            const auto evaluate = [&](int block, int begin, int end) -> Result<void> {
                partials[block] = end - begin;
                return {};
            };
            for (auto _ : state) {
                for (int pass = 0; pass < 96; ++pass) {
                    auto result = reuse ? blocks.run(evaluate) : detail::monte_carlo_blocks(4096, 4, evaluate);
                    benchmark::DoNotOptimize(result);
                }
                benchmark::DoNotOptimize(partials);
            }
        })->UseRealTime();
    using namespace kiyosi;
    for (const int paths : {4096, 250'000})
        for (const int threads : {1, 0}) {
            const auto [option, context] = monte_carlo_scenario();
            const auto engine = *make_monte_carlo_vanilla_engine({paths, 50, 42, MonteCarloBackend::cpu, threads});
            for (const bool all : {false, true})
                register_calculation(std::format("european_samples/{}/threads:{}/{}", paths, threads, all ? "all" : "delta_gamma").c_str(), [=] { return engine.price_with_greeks(option, context, all ? GreeksRequest{true} : GreeksRequest{Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
        }
    const Date start{std::chrono::year{2025} / 1 / 1};
    for (const int days : {1, 12}) {
        const auto expiry = start + std::chrono::days{days};
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                                   start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
        const auto accumulator = *make_accumulator({100.0, 110.0, 1.0, 2.0, 0.0, start, expiry});
        const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.08}}});
        for (const int paths : {256, 257, 512, 1024, 2000})
            for (const int threads : {1, 0, 2}) {
                const auto suffix = std::format("/{}/days:{}/threads:{}", paths, days, threads);
                register_price(("structured_short/accumulator" + suffix).c_str(), accumulator,
                               *make_monte_carlo_accumulator_engine({paths, 42, MonteCarloBackend::cpu, threads}), context)
                    ->UseRealTime();
                register_price(("structured_short/binary_snowball" + suffix).c_str(), note,
                               *make_monte_carlo_autocallable_engine({paths, 42, MonteCarloBackend::cpu, threads}), context)
                    ->UseRealTime();
            }
    }
    for (const int threads : {1, 0}) {
        const auto [option, context] = american_monte_carlo_scenario();
        const auto engine = make_monte_carlo_vanilla_engine({100'000, 50, 42, MonteCarloBackend::cpu, threads}).value();
        for (const bool mixed : {false, true})
            for (const bool shared : {false, true})
                register_calculation(std::format("american_samples/100000/threads:{}/{}{}", threads, shared ? "shared" : "scalar", mixed ? "/mixed" : "").c_str(), [=] {
                    const GreeksRequest request = mixed ? GreeksRequest{Greek::delta, Greek::vega, Greek::rho} : GreeksRequest{Greek::vega, Greek::rho};
                    return shared ? engine.price_with_greeks(option, context, request)
                                  : detail::price_with_greeks(engine, option, context, request, NumericalShiftSettings{}); }, "price")->UseRealTime();
    }
    for (const int paths : {512, 1024, 2048, 2049, 2560, 3072, 3073, 3584, 4096})
        for (const int threads : {1, 0, 2})
            register_monte_carlo(
                std::format("european_terminal/{}/threads:{}", paths, threads),
                monte_carlo_scenario(),
                make_monte_carlo_vanilla_engine({paths, 50, 42, MonteCarloBackend::cpu, threads}).value());
    for (const auto type : {OptionType::put, OptionType::call})
        for (const int paths : {128, 512, 1024, 4096, 250'000, 1'000'000})
            for (const int threads : {1, 2, 4, 12, 0})
                register_monte_carlo(
                    std::format("american_lsm/{}/{}/threads:{}", type == OptionType::put ? "put" : "call", paths, threads),
                    american_monte_carlo_scenario(type),
                    make_monte_carlo_vanilla_engine({paths, 50, 42, MonteCarloBackend::cpu, threads}).value());
    for (const auto backend : {MonteCarloBackend::cpu
#if KIYOSI_HAS_CUDA
                               ,
                               MonteCarloBackend::cuda
#endif
         }) {
        for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
            if (backend == MonteCarloBackend::cuda && sampling == MonteCarloSampling::sobol) continue;
            for (const bool antithetic : {false, true}) {
                const auto mode = std::format("antithetic/{}/{}/{}/", backend == MonteCarloBackend::cpu ? "cpu" : "cuda", sampling == MonteCarloSampling::sobol ? "sobol" : "pseudo_random", antithetic ? "on" : "off");
                const auto vanilla = make_monte_carlo_vanilla_engine({32'768, 50, 42, backend, 0, sampling, antithetic}).value();
                const TradingDayMonteCarloSettings trading{32'768, 42, backend, 0, sampling, antithetic};
                register_monte_carlo(mode + "european", monte_carlo_scenario(), vanilla);
                register_monte_carlo(mode + "american", american_monte_carlo_scenario(), vanilla);
                register_monte_carlo(mode + "accumulator", accumulator_scenario(), make_monte_carlo_accumulator_engine(trading).value());
                const auto notes = make_monte_carlo_autocallable_engine(trading).value();
                register_monte_carlo(mode + "phoenix", phoenix_scenario(), notes);
                register_monte_carlo(mode + "snowball", snowball_scenario(), notes);
            }
        }
        const std::string suffix = backend == MonteCarloBackend::cpu ? "_cpu" : "_cuda";
        const auto vanilla = make_monte_carlo_vanilla_engine({1'000'000, 50, 42, backend}).value();
        register_monte_carlo("bm_monte_carlo_european" + suffix, monte_carlo_scenario(), vanilla);
        register_monte_carlo("bm_monte_carlo_american" + suffix, american_monte_carlo_scenario(), vanilla);
        register_monte_carlo("bm_monte_carlo_accumulator" + suffix, accumulator_scenario(),
                             make_monte_carlo_accumulator_engine({250'000, 42, backend}).value());
        register_monte_carlo("bm_monte_carlo_phoenix" + suffix, phoenix_scenario(),
                             make_monte_carlo_autocallable_engine({250'000, 42, backend}).value());
        register_monte_carlo("bm_monte_carlo_snowball" + suffix, snowball_scenario(),
                             make_monte_carlo_autocallable_engine({250'000, 42, backend}).value());
    }
    for (const int threads : {1, 0}) {
        const std::string mode = threads == 1 ? "/serial" : "/automatic";
        const auto add = [&](const std::string& name, const auto& scenario, const auto& engine) {
            const auto& [option, context] = scenario;
            register_monte_carlo(std::format("parallel/{}{}/price", name, mode), scenario, engine);
            register_calculation(std::format("parallel/{}{}/greeks", name, mode).c_str(), [=, option = option] { return engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::rho}); }, "price")->UseRealTime();
        };
        for (const int paths : {128, 250'000}) {
            const auto size = std::to_string(paths);
            const auto vanilla = make_monte_carlo_vanilla_engine({paths, 50, 42, MonteCarloBackend::cpu, threads}).value();
            add("european/" + size, monte_carlo_scenario(), vanilla);
            add("american/" + size, american_monte_carlo_scenario(), vanilla);
            add("accumulator/" + size, accumulator_scenario(),
                make_monte_carlo_accumulator_engine({paths, 42, MonteCarloBackend::cpu, threads}).value());
            const auto exotic = make_monte_carlo_autocallable_engine({paths, 42, MonteCarloBackend::cpu, threads}).value();
            add("phoenix/" + size, phoenix_scenario(), exotic);
            add("snowball/" + size, snowball_scenario(), exotic);
            const auto [option, context] = monte_carlo_scenario();
            const auto quote = vanilla.price(option, context);
            register_calculation(std::format("parallel/european/{}{}/implied_volatility", size, mode).c_str(), [=, quote = quote]() -> Result<double> {
                if (!quote) return std::unexpected(quote.error());
                return implied_volatility(vanilla, option, context, *quote, {.lower_bound = 0.1, .upper_bound = 0.4}); }, "volatility")->UseRealTime();
        }
    }
}
