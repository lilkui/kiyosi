#include <chrono>
#include <cstdio>
#include <exception>
#include <string>
#include <vector>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"

namespace {

template <typename Option, typename Engine>
void register_kiyosi(const char* name, Option option, Engine engine, kiyosi::PricingContext context,
                     bool comparable = true, bool use_real_time = false)
{
    const std::string full_name = std::string{comparable ? "matrix/kiyosi/" : "matrix/kiyosi_only/"} + name;
    auto* registered = kiyosi::benchmark_support::register_price(
        full_name.c_str(), std::move(option), std::move(engine), std::move(context));
    if (use_real_time) registered->UseRealTime();
}

void register_matrix()
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date expiry{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const auto european = *make_european_option(OptionType::call, 100.0, start, expiry);
    const auto american = *make_american_option(OptionType::put, 100.0, start, expiry);
    const auto cash_digital = *make_cash_or_nothing_option(OptionType::call, 100.0, 10.0, start, expiry);
    const auto asset_digital = *make_asset_or_nothing_option(OptionType::call, 100.0, start, expiry);
    const auto barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out});
    const BinaryBarrierTerms binary_terms{.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out};
    const auto cash_binary = *make_cash_binary_barrier_option(binary_terms, 10.0);
    const auto asset_binary = *make_asset_binary_barrier_option(binary_terms);
    const auto cash_touch = *make_cash_one_touch_up(start, expiry, 120.0, 10.0);
    const auto asset_touch = *make_asset_one_touch_up(start, expiry, 120.0);
    const auto geometric = *make_geometric_average_option(OptionType::call, 100.0, start, start, expiry);
    const auto arithmetic = *make_arithmetic_average_option(OptionType::call, 100.0, start, start, expiry);
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 110.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = expiry});
    const auto phoenix = *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .effective_date = start, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.0, 90.0}}});
    const auto snowball = *make_both_down_snowball({.initial_coupon_rate = 0.08,
                                                    .coupon_rate_decrement = 0.01,
                                                    .initial_spot = 100.0,
                                                    .knock_in_level = 80.0,
                                                    .initial_knock_out_level = 110.0,
                                                    .knock_out_level_decrement = 5.0,
                                                    .observation_dates = {expiry},
                                                    .effective_date = start,
                                                    .expiry_date = expiry});
    const auto binary_snowball = *make_binary_snowball_option({.maturity_coupon_rate = 0.05, .effective_date = start, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.08}}});
    const auto ternary_snowball = *make_ternary_snowball_option({.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .effective_date = start, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.08}}});

    register_kiyosi("european/analytic", european, AnalyticVanillaEngine{}, context);
    const auto up_and_out = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out});
    register_kiyosi("up_and_out/fd_200", up_and_out, FiniteDifferenceBarrierEngine{200, 200}, context);
    register_kiyosi("up_and_out/fd_2000", up_and_out, FiniteDifferenceBarrierEngine{2'000, 200}, context);
    const Date dense_expiry{std::chrono::year{2035} / 1 / 1};
    std::vector<Date> dense_dates;
    for (auto date = start + std::chrono::days{1}; date <= dense_expiry; date += std::chrono::days{1})
        dense_dates.push_back(date);
    const auto dense_note = *make_snowball_option({.maturity_coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = dense_expiry, .observations = *kiyosi::make_autocallable_observations(dense_dates, std::vector<double>(dense_dates.size(), 110.0), std::vector<double>(dense_dates.size(), 0.08))});
    const auto dense_context = *make_pricing_context(context.model_parameters(), 80.25,
                                                     start_of_day(dense_expiry) - std::chrono::hours{12}, all_days_calendar());
    const FiniteDifferenceAutocallableEngine dense_engine{80, 1};
    const double dense_quote = *dense_engine.price(dense_note, dense_context);
    kiyosi::benchmark_support::register_calculation("snowball/fd/implied_volatility/scheduled_dense", [=, dense_note = dense_note] { return implied_volatility(dense_engine, dense_note, dense_context, dense_quote); }, "volatility");
    kiyosi::benchmark_support::register_calculation("snowball/fd/implied_volatility/scheduled_dense/independent_trials", [=, dense_note = dense_note] { return detail::solve_implied_volatility(dense_engine, dense_note, dense_context, dense_quote, {},
                                                                                                                                                                                                [](const auto& engine, const auto& note, const auto& market) { return engine.price(note, market); }); }, "volatility");
    register_kiyosi("european/crr", european, CoxRossRubinsteinVanillaEngine{128}, context);
    register_kiyosi("european/fd", european, FiniteDifferenceVanillaEngine{80, 80}, context);
    register_kiyosi("european/quadrature", european, QuadratureVanillaEngine{}, context);
    register_kiyosi("european/mc", european, MonteCarloVanillaEngine{5'000, 10, 42}, context);
    register_kiyosi("american/bjerksund_stensland", american, BjerksundStenslandVanillaEngine{}, context);
    register_kiyosi("american/crr", american, CoxRossRubinsteinVanillaEngine{128}, context);
    register_kiyosi("american/fd", american, FiniteDifferenceVanillaEngine{80, 80}, context);
    register_kiyosi("american/mc", american, MonteCarloVanillaEngine{5'000, 20, 42}, context);
    register_kiyosi("cash_digital/analytic", cash_digital, AnalyticDigitalEngine{}, context);
    register_kiyosi("cash_digital/fd", cash_digital, FiniteDifferenceDigitalEngine{80, 80}, context);
    register_kiyosi("cash_digital/quadrature", cash_digital, QuadratureDigitalEngine{}, context);
    register_kiyosi("asset_digital/analytic", asset_digital, AnalyticDigitalEngine{}, context);
    register_kiyosi("asset_digital/fd", asset_digital, FiniteDifferenceDigitalEngine{80, 80}, context);
    register_kiyosi("asset_digital/quadrature", asset_digital, QuadratureDigitalEngine{}, context);
    register_kiyosi("barrier/analytic", barrier, AnalyticBarrierEngine{}, context);
    register_kiyosi("barrier/fd", barrier, FiniteDifferenceBarrierEngine{80, 80}, context);
    register_kiyosi("cash_binary_barrier/analytic", cash_binary, AnalyticBinaryBarrierEngine{}, context);
    register_kiyosi("asset_binary_barrier/analytic", asset_binary, AnalyticBinaryBarrierEngine{}, context);
    register_kiyosi("cash_touch/analytic", cash_touch, AnalyticBinaryBarrierEngine{}, context);
    register_kiyosi("asset_touch/analytic", asset_touch, AnalyticBinaryBarrierEngine{}, context);
    register_kiyosi("geometric_asian/analytic", geometric, AnalyticGeometricAveragePriceEngine{}, context);
    register_kiyosi("arithmetic_asian/approximation", arithmetic,
                    TurnbullWakemanArithmeticAveragePriceEngine{}, context);
    register_kiyosi("accumulator/fd", accumulator, FiniteDifferenceAccumulatorEngine{80, 80}, context, false);
    register_kiyosi("accumulator/mc", accumulator, MonteCarloAccumulatorEngine{2'000, 42}, context, false);
    register_kiyosi("phoenix/fd", phoenix, FiniteDifferenceAutocallableEngine{80, 80}, context, false);
    register_kiyosi("phoenix/mc", phoenix, MonteCarloAutocallableEngine{2'000, 42}, context, false);
    register_kiyosi("snowball/fd", snowball, FiniteDifferenceAutocallableEngine{80, 80}, context, false);
    register_kiyosi("snowball/mc", snowball, MonteCarloAutocallableEngine{2'000, 42}, context, false);
    register_kiyosi("binary_snowball/fd", binary_snowball, FiniteDifferenceAutocallableEngine{80, 80}, context, false);
    register_kiyosi("binary_snowball/mc", binary_snowball, MonteCarloAutocallableEngine{2'000, 42}, context, false);
    register_kiyosi("ternary_snowball/fd", ternary_snowball, FiniteDifferenceAutocallableEngine{80, 80}, context, false);
    register_kiyosi("ternary_snowball/mc", ternary_snowball, MonteCarloAutocallableEngine{2'000, 42}, context, false);
#if KIYOSI_HAS_CUDA
    register_kiyosi("european/mc_cuda", european, MonteCarloVanillaEngine{5'000, 10, 42, MonteCarloBackend::cuda}, context, false, true);
    register_kiyosi("american/mc_cuda", american, MonteCarloVanillaEngine{5'000, 20, 42, MonteCarloBackend::cuda}, context, false, true);
    register_kiyosi("accumulator/mc_cuda", accumulator, MonteCarloAccumulatorEngine{2'000, 42, MonteCarloBackend::cuda}, context, false, true);
    register_kiyosi("phoenix/mc_cuda", phoenix, MonteCarloAutocallableEngine{2'000, 42, MonteCarloBackend::cuda}, context, false, true);
    register_kiyosi("snowball/mc_cuda", snowball, MonteCarloAutocallableEngine{2'000, 42, MonteCarloBackend::cuda}, context, false, true);
    register_kiyosi("binary_snowball/mc_cuda", binary_snowball, MonteCarloAutocallableEngine{2'000, 42, MonteCarloBackend::cuda}, context, false, true);
    register_kiyosi("ternary_snowball/mc_cuda", ternary_snowball, MonteCarloAutocallableEngine{2'000, 42, MonteCarloBackend::cuda}, context, false, true);
#endif
}

} // namespace

int main(int argc, char** argv)
{
    try {
        benchmark::MaybeReenterWithoutASLR(argc, argv);
        benchmark::Initialize(&argc, argv);
        if (benchmark::ReportUnrecognizedArguments(argc, argv)) return 1;
        register_matrix();
        kiyosi::benchmark_support::register_monte_carlo_cases();
        kiyosi::benchmark_support::register_analytics_cases();
#if KIYOSI_HAS_QUANTLIB
        kiyosi::benchmark_support::register_quantlib_matrix();
#endif
        benchmark::RunSpecifiedBenchmarks();
        benchmark::Shutdown();
        return 0;
    } catch (const std::exception& error) {
        std::fputs(error.what(), stderr);
        std::fputc('\n', stderr);
        return 1;
    }
}
