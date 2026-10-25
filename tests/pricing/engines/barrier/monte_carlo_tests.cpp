#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <cmath>
#include <future>
#include <numbers>

#include <kiyosi/kiyosi.hpp>
#include "pricing/engines/monte_carlo_host.hpp"
#include "pricing/engines/monte_carlo_barrier.hpp"
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using test::day;
constexpr auto start = day(2025, 1, 1), end = day(2026, 1, 1);

BarrierOption barrier(BarrierType kind, OptionType direction = OptionType::call, double rebate = 2.0,
                      RebateTiming timing = RebateTiming::at_expiry, std::vector<Date> dates = {})
{
    const auto mode = dates.empty() ? ObservationMode::continuous : ObservationMode::scheduled;
    return make_barrier_option({.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = is_up_barrier(kind) ? 120.0 : 80.0, .barrier_type = kind, .rebate = rebate, .rebate_timing = timing, .observation_mode = mode, .observation_dates = std::move(dates)}).value();
}

PricingContext market(double spot = 100.0, double rate = 0.04)
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
    return make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.2), spot, start).value();
}
} // namespace

TEST_CASE("Barrier MC absorbing-density prices and Greeks agree with analytic values", "[barrier-mc]")
{
    const auto context = market();
    for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
        const auto engine = make_monte_carlo_barrier_engine({.path_count = 131'072, .seed = 42, .sampling = sampling}).value();
        for (const auto kind : {BarrierType::up_and_in, BarrierType::up_and_out, BarrierType::down_and_in, BarrierType::down_and_out})
            for (const auto direction : {OptionType::call, OptionType::put}) {
                CAPTURE(sampling, kind, direction);
                const auto option = barrier(kind, direction);
                const auto actual = engine.price_with_greeks(option, context, true);
                const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                REQUIRE(actual);
                REQUIRE(expected);
                CHECK(actual->price() == Catch::Approx(expected->price()).margin(0.12));
                CHECK(*actual->require(Greek::delta) == Catch::Approx(*expected->require(Greek::delta)).margin(0.04));
                CHECK(*actual->require(Greek::gamma) == Catch::Approx(*expected->require(Greek::gamma)).margin(0.012));
                CHECK(actual->price() == *engine.price(option, context));
                for (const auto greek : {Greek::vega, Greek::rho, Greek::theta, Greek::speed, Greek::vanna, Greek::zomma, Greek::charm, Greek::color})
                    CHECK_FALSE(actual->has(greek));
            }
    }
}

TEST_CASE("Barrier MC continuous hit rebates retain negative-rate value and derivatives", "[barrier-mc]")
{
    const auto engine = make_monte_carlo_barrier_engine({.path_count = 65'536, .seed = 17, .sampling = MonteCarloSampling::sobol}).value();
    for (const auto kind : {BarrierType::up_and_out, BarrierType::down_and_out})
        for (const double rate : {-0.02, 0.04}) {
            const auto option = barrier(kind, OptionType::call, 10.0, RebateTiming::at_hit);
            const auto context = market(100.0, rate);
            const auto actual = engine.price_with_greeks(option, context, true);
            const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(actual);
            REQUIRE(expected);
            CHECK(actual->price() == Catch::Approx(expected->price()).margin(0.015));
            CHECK(*actual->require(Greek::delta) == Catch::Approx(*expected->require(Greek::delta)).margin(0.006));
            CHECK(*actual->require(Greek::gamma) == Catch::Approx(*expected->require(Greek::gamma)).margin(0.003));
        }
}

TEST_CASE("Barrier MC scheduled likelihood Greeks and early monitoring match event-date pricing", "[barrier-mc]")
{
    const auto context = market();
    const auto engine = make_monte_carlo_barrier_engine({.path_count = 131'072, .seed = 42, .sampling = MonteCarloSampling::sobol}).value();
    const auto reference = make_finite_difference_barrier_engine({600, 800}).value();
    for (const bool early : {false, true})
        for (const auto kind : {BarrierType::up_and_in, BarrierType::up_and_out, BarrierType::down_and_in, BarrierType::down_and_out}) {
            auto dates = std::vector{start + std::chrono::days{90}, start + std::chrono::days{180}};
            if (!early) dates.push_back(end);
            const auto option = barrier(kind, OptionType::put, 2.0, RebateTiming::at_expiry, std::move(dates));
            const auto actual = engine.price_with_greeks(option, context, true);
            const auto expected = reference.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(actual);
            REQUIRE(expected);
            CHECK(actual->price() == Catch::Approx(expected->price()).margin(0.08));
            CHECK(*actual->require(Greek::delta) == Catch::Approx(*expected->require(Greek::delta)).margin(0.03));
            CHECK(*actual->require(Greek::gamma) == Catch::Approx(*expected->require(Greek::gamma)).margin(0.015));
        }
}

TEST_CASE("Barrier MC reproduces seeded estimates across threads steps pairing and concurrent calls", "[barrier-mc]")
{
    const auto context = market();
    const auto option = barrier(BarrierType::down_and_out);
    for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol})
        for (const bool paired : {false, true}) {
            MonteCarloSettings settings{.path_count = 1024, .seed = 17, .thread_count = 1, .sampling = sampling, .antithetic = paired};
            const auto engine = make_monte_carlo_barrier_engine(settings).value();
            const auto expected = engine.price_with_greeks(option, context, true).value();
            settings.thread_count = 4;
            settings.step_count = 10'000;
            const auto parallel = make_monte_carlo_barrier_engine(settings).value();
            const auto actual = parallel.price_with_greeks(option, context, true).value();
            CHECK(actual.price() == expected.price());
            CHECK(actual.values_view() == expected.values_view());
            auto other = std::async(std::launch::async, [&] { return parallel.price(option, context); });
            CHECK(*parallel.price(option, context) == expected.price());
            CHECK(*other.get() == expected.price());
        }
}

TEST_CASE("Barrier MC respects exact odd antithetic path counts", "[barrier-mc]")
{
    const auto context = market();
    for (const int paths : {1, 2, 3, 5, 257}) {
        const auto engine = make_monte_carlo_barrier_engine({.path_count = paths, .seed = 42, .antithetic = true}).value();
        const auto in = barrier(BarrierType::down_and_in, OptionType::call, 0.0);
        const auto out = barrier(BarrierType::down_and_out, OptionType::call, 0.0);
        const auto sum = *engine.price(in, context) + *engine.price(out, context);
        double expected = 0.0;
        const int samples = (paths + 1) / 2;
        for (int path = 0; path < samples; ++path) {
            detail::Pcg32 generator{42, static_cast<std::uint64_t>(path)};
            std::normal_distribution<double> normal;
            const double z = normal(generator);
            const int sides = path < paths / 2 ? 2 : 1;
            for (int side = 0; side < sides; ++side)
                expected += std::max(100.0 * std::exp(0.01 + 0.2 * (side == 0 ? z : -z)) - 100.0, 0.0) * std::exp(-0.04);
        }
        CHECK(sum == Catch::Approx(expected / paths).margin(1e-12));
    }
}

TEST_CASE("Barrier MC retains scaled rare-hit terminal contributions", "[barrier-mc]")
{
    const double spot = 1e300;
    const detail::BarrierMonteCarloParameters parameters{
        spot, 1.0, spot * std::exp(-20.0), std::log(spot), 20.0, 0.0, {1.0, 0.0}, true, false, true, false, true};
    const auto sample = parameters.sample(std::log(spot), 0.0, 1.0, false, 0.0, false, false);
    const double distance = std::log(spot) - std::log(parameters.barrier);
    const double expected = std::exp(std::log(spot) - 40.0 * distance);
    CHECK(sample.price == Catch::Approx(expected).epsilon(1e-12));
    CHECK(sample.price > 0.0);
}

TEST_CASE("Barrier MC scheduled CPU hit rebates use observation-time discounting", "[barrier-mc]")
{
    const auto context = market();
    const Date observation = start + std::chrono::days{180};
    const auto engine = make_monte_carlo_barrier_engine({.path_count = 131'072, .seed = 42}).value();
    // Rebate differences cancel the surviving payoff on the shared seeded paths.
    const auto option = barrier(BarrierType::up_and_out, OptionType::put, 10.0, RebateTiming::at_hit, {observation});
    const auto zero = barrier(BarrierType::up_and_out, OptionType::put, 0.0, RebateTiming::at_hit, {observation});
    const double time = 180.0 / 365.0;
    const double threshold = (std::log(120.0 / 100.0) - 0.01 * time) / (0.2 * std::sqrt(time));
    const double probability = 0.5 * std::erfc(threshold / std::numbers::sqrt2);
    const double expected = 10.0 * std::exp(-0.04 * time) * probability;
    CHECK(*engine.price(option, context) - *engine.price(zero, context) == Catch::Approx(expected).margin(0.025));
    const auto seeded = detail::with_monte_carlo_seed<MonteCarloBarrierEngine, BarrierOption>(
        MonteCarloBarrierEngine{}, [&](const auto& request) -> Result<bool> {
            CHECK(request.settings().seed.has_value());
            return request.price(option, context).value() == request.price(option, context).value();
        });
    REQUIRE(seeded);
    CHECK(*seeded);
}

TEST_CASE("Barrier MC validates history and resolves current touches before backend execution", "[barrier-mc]")
{
    const auto engine = make_monte_carlo_barrier_engine({.backend = MonteCarloBackend::cuda}).value();
    const auto option = barrier(BarrierType::up_and_out, OptionType::call, 10.0, RebateTiming::at_hit);
    const auto current = engine.price_with_greeks(option, market(120.0), true);
    REQUIRE(current);
    CHECK(current->price() == 10.0);
    CHECK_FALSE(current->has(Greek::delta));
    const auto later = market().replace({.valuation_time = start_of_day(start) + std::chrono::hours{1}}).value();
    const auto missing = engine.price(option, later);
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == ErrorCategory::invalid_parameter);
    const auto touched = option.replace(BarrierTouchState::touched).value();
    CHECK(*engine.price(touched, later) == 0.0);
    const auto expired = market().replace({.valuation_time = start_of_day(end)}).value();
    const auto final = engine.price_with_greeks(touched, expired, true);
    REQUIRE(final);
    CHECK(final->price() == 0.0);
    CHECK_FALSE(final->has(Greek::gamma));
    const auto untouched = barrier(BarrierType::up_and_in).replace(BarrierTouchState::untouched).value();
    const auto unactivated = engine.price_with_greeks(untouched, expired, true);
    REQUIRE(unactivated);
    CHECK(unactivated->price() == 2.0);
    CHECK_FALSE(unactivated->has(Greek::delta));
    const auto invalid = make_monte_carlo_barrier_engine({.path_count = 0});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("Barrier MC implied volatility shares a request-local price curve", "[barrier-mc]")
{
    const auto engine = make_monte_carlo_barrier_engine({.path_count = 8192, .seed = 42, .sampling = MonteCarloSampling::sobol}).value();
    const auto option = barrier(BarrierType::down_and_out, OptionType::call, 0.0);
    const auto context = market();
    const double quote = *engine.price(option, context);
    const auto solved = implied_volatility(engine, option, context, quote, {.lower_bound = 0.15, .upper_bound = 0.25});
    REQUIRE(solved);
    CHECK(*solved == Catch::Approx(0.2).margin(1e-5));
}

TEST_CASE("Barrier MC CUDA prices and native Greeks agree with analytic references", "[barrier-mc][cuda]")
{
    const auto engine = make_monte_carlo_barrier_engine({.path_count = 262'144, .seed = 42, .backend = MonteCarloBackend::cuda, .antithetic = true}).value();
    for (const bool scheduled : {false, true}) {
        const auto option = barrier(BarrierType::down_and_out, OptionType::put, 2.0, RebateTiming::at_hit,
                                    scheduled ? std::vector{start + std::chrono::days{180}, end} : std::vector<Date>{});
        const auto actual = engine.price_with_greeks(option, market(), true);
        if (!actual && actual.error().category == ErrorCategory::backend_unavailable) {
            SUCCEED("CUDA backend unavailable; public failure category verified");
            return;
        }
        REQUIRE(actual);
        const auto expected = scheduled ? make_finite_difference_barrier_engine({600, 800})->price_with_greeks(option, market(), {Greek::delta, Greek::gamma})
                                        : AnalyticBarrierEngine{}.price_with_greeks(option, market(), {Greek::delta, Greek::gamma});
        REQUIRE(expected);
        CHECK(actual->price() == Catch::Approx(expected->price()).margin(0.10));
        CHECK(*actual->require(Greek::delta) == Catch::Approx(*expected->require(Greek::delta)).margin(0.03));
        CHECK(*actual->require(Greek::gamma) == Catch::Approx(*expected->require(Greek::gamma)).margin(0.015));
    }
}
