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

auto snowball_scenario()
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
                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
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
    const auto context = *make_pricing_context(
        base_context.model_parameters(), base_context.spot_price(),
        base_context.valuation_time() + std::chrono::hours{12}, base_context.calendar());
    const auto accumulator = accumulator_scenario().first;
    const auto snowball = snowball_scenario().first;
    const FiniteDifferenceAccumulatorEngine accumulator_engine{};
    const FiniteDifferenceSnowballEngine snowball_engine{};
    register_price("analytics/kiyosi/accumulator/fd/price", accumulator, accumulator_engine, context);
    register_calculation("analytics/kiyosi/accumulator/fd/delta_gamma", [=] { return accumulator_engine.price_with_greeks(accumulator, context, {Greek::delta, Greek::gamma}); }, "price");
    register_price("analytics/kiyosi/snowball/fd/price", snowball, snowball_engine, context);
    register_calculation("analytics/kiyosi/snowball/fd/delta_gamma", [=] { return snowball_engine.price_with_greeks(snowball, context, {Greek::delta, Greek::gamma}); }, "price");
    const MonteCarloVanillaEngine monte_carlo{{5'000, 10, 42}};
    const auto quote = monte_carlo.price(european, context);
    register_calculation("analytics/kiyosi/european/mc/implied_volatility", [=]() -> Result<double> {
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
