#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <cmath>
#include <future>
#include <limits>
#include <random>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include "pricing/engines/monte_carlo_host.hpp"
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using test::day;
using test::greek_value;

constexpr auto start = day(2025, 1, 1), end = day(2026, 1, 1);

PricingContext market(double spot = 100.0, double volatility = 0.2, Timestamp valuation = start_of_day(start))
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
    return make_pricing_context(*make_bsm_parameters(0.05, 0.02, volatility), spot, valuation).value();
}

template <typename Check>
void digital_options(const Check& check, double strike = 100.0)
{
    for (const auto type : {OptionType::call, OptionType::put}) {
        check(*make_cash_or_nothing_option(type, strike, 10.0, start, end), false);
        check(*make_asset_or_nothing_option(type, strike, start, end), true);
    }
}
} // namespace

TEST_CASE("Digital MC price and likelihood Greeks agree with analytic values", "[digital-numerical][digital-mc]")
{
    for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
        const int paths = 65'536;
        const auto engine = *make_monte_carlo_digital_engine({.path_count = paths, .seed = 42, .sampling = sampling});
        for (const int remaining : {1, 365})
            for (const double spot : {95.0, 100.0, 105.0}) {
                const auto context = market(spot, 0.2, start_of_day(end - std::chrono::days{remaining}));
                const double time = remaining / 365.0, width = 0.2 * std::sqrt(time);
                digital_options([&](const auto& option, bool asset) {
                    CAPTURE(sampling, remaining, spot, asset, option.option_type());
                    const auto actual = engine.price_with_greeks(option, context, true);
                    const auto expected = AnalyticDigitalEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                    REQUIRE(actual);
                    REQUIRE(expected);
                    // Bounds use full-payoff second moments, covering either in-the-money event.
                    const double second = asset ? spot * spot * std::exp(-0.04 * time + width * width)
                                                : 100.0 * std::exp(-0.1 * time);
                    const double price_error = 6.0 * std::sqrt(second / paths);
                    const double delta_error = 6.0 * std::sqrt(second * (asset ? 1.0 + 4.0 * width * width : 1.0) / paths) / spot / width;
                    const double b2 = width * width;
                    const double gamma_error = 6.0 * std::sqrt(second * (asset ? 2.0 + 9.0 * b2 + 4.0 * b2 * b2 : 2.0 + b2) / paths) / spot / spot / b2;
                    CHECK(actual->price() == Catch::Approx(expected->price()).margin(price_error));
                    CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(greek_value(*expected, Greek::delta)).margin(delta_error));
                    CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(greek_value(*expected, Greek::gamma)).margin(gamma_error));
                    CHECK(actual->price() == *engine.price(option, context));
                    for (std::size_t index = 0; index < greek_count; ++index) {
                        const auto greek = static_cast<Greek>(index);
                        CHECK(actual->has(greek) == (greek == Greek::delta || greek == Greek::gamma));
                    }
                });
            }
    }
}

TEST_CASE("Digital MC uses exact seeded path counts and antithetic likelihood scores", "[digital-numerical][digital-mc]")
{
    const auto context = market();
    for (const bool antithetic : {false, true})
        for (const int paths : {1, 2, 3, 4, 5, 257}) {
            const auto engine = *make_monte_carlo_digital_engine({.path_count = paths, .seed = 42, .antithetic = antithetic});
            digital_options([&](const auto& option, bool asset) {
                CAPTURE(paths, antithetic, asset, option.option_type());
                std::array<double, 3> sums{};
                const int count = antithetic ? (paths + 1) / 2 : paths;
                for (int path = 0; path < count; ++path) {
                    detail::Pcg32 generator{42, static_cast<std::uint64_t>(path)};
                    std::normal_distribution<double> normal;
                    const double draw = normal(generator);
                    const int paired = antithetic && path < paths / 2 ? 2 : 1;
                    for (int side = 0; side < paired; ++side) {
                        const double z = side == 0 ? draw : -draw;
                        const double terminal = 100.0 * std::exp(0.01 + 0.2 * z);
                        const bool pays = option.option_type() == OptionType::call ? terminal > 100.0 : terminal < 100.0;
                        const double y = pays ? (asset ? terminal : 10.0) * std::exp(-0.05) : 0.0;
                        sums[0] += y;
                        sums[1] += y * z / 20.0;
                        sums[2] += y * (z * z - 1.0 - 0.2 * z) / 400.0;
                    }
                }
                const auto actual = engine.price_with_greeks(option, context, true);
                REQUIRE(actual);
                CHECK(actual->price() == Catch::Approx(sums[0] / paths).margin(1e-12));
                CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(sums[1] / paths).margin(1e-12));
                CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(sums[2] / paths).margin(1e-12));
            });
        }
}

TEST_CASE("Digital MC shares results across threads steps selected Greeks and concurrent calls", "[digital-numerical][digital-mc]")
{
    const auto context = market();
    for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol})
        for (const bool antithetic : {false, true}) {
            MonteCarloSettings settings{.path_count = sampling == MonteCarloSampling::sobol ? 1024 : 1025,
                                        .seed = 42,
                                        .sampling = sampling,
                                        .antithetic = antithetic};
            digital_options([&](const auto& option, bool) {
                const auto engine = *make_monte_carlo_digital_engine(settings);
                const auto expected = engine.price_with_greeks(option, context, true).value();
                for (const int threads : {0, 1, 4}) {
                    auto changed = settings;
                    changed.step_count = 10'000;
                    changed.thread_count = threads;
                    const auto other = *make_monte_carlo_digital_engine(changed);
                    for (const auto greek : {Greek::delta, Greek::gamma, Greek::rho}) {
                        const auto actual = other.price_with_greeks(option, context, {greek});
                        REQUIRE(actual);
                        CHECK(actual->price() == expected.price());
                        CHECK(*actual->get(greek) == *expected.get(greek));
                        CHECK_FALSE(actual->has(greek == Greek::delta ? Greek::gamma : Greek::delta));
                    }
                }
                auto first = std::async(std::launch::async, [&] { return engine.price(option, context); });
                CHECK(*engine.price(option, context) == expected.price());
                CHECK(*first.get() == expected.price());
            });
        }
}

TEST_CASE("Digital CRR prices and node Greeks match explicit rollback", "[digital-numerical][digital-crr]")
{
    for (const int steps : {1, 2, 3, 17, 255, 256, 257}) {
        const auto engine = *make_cox_ross_rubinstein_digital_engine({steps});
        const double log_up = 0.2 / std::sqrt(steps), up = std::exp(log_up), down = 1.0 / up;
        const double p = std::expm1(0.03 / steps + log_up) / std::expm1(2.0 * log_up);
        const double discount = std::exp(-0.05 / steps);
        digital_options([&](const auto& option, bool asset) {
            CAPTURE(steps, asset, option.option_type());
            std::vector<double> values(static_cast<std::size_t>(steps) + 1);
            for (int node = 0; node <= steps; ++node) {
                const double terminal = 100.0 * std::exp((2.0 * node - steps) * log_up);
                const bool pays = option.option_type() == OptionType::call ? terminal > 100.0 : terminal < 100.0;
                values[node] = pays ? (asset ? terminal : 10.0) : 0.0;
            }
            std::array<double, 2> first{};
            std::array<double, 3> second{};
            for (int level = steps; level > 0; --level) {
                if (level == 1) first = {values[0], values[1]};
                if (level == 2) second = {values[0], values[1], values[2]};
                for (int node = 0; node < level; ++node)
                    values[node] = discount * ((1.0 - p) * values[node] + p * values[node + 1]);
            }
            const auto actual = engine.price_with_greeks(option, market(), true);
            REQUIRE(actual);
            CHECK(actual->price() == Catch::Approx(values[0]).margin(1e-10));
            CHECK(greek_value(*actual, Greek::delta) == Catch::Approx((first[1] - first[0]) / (100.0 * (up - down))).margin(1e-10));
            CHECK(actual->has(Greek::gamma) == (steps >= 2));
            if (steps >= 2) {
                const double gamma = ((second[2] - second[1]) / (100.0 * (up * up - 1.0)) -
                                      (second[1] - second[0]) / (100.0 * (1.0 - down * down))) /
                                     (50.0 * (up * up - down * down));
                CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(gamma).margin(1e-10));
                const auto gamma_only = engine.price_with_greeks(option, market(), {Greek::gamma}).value();
                CHECK(greek_value(gamma_only, Greek::gamma) == greek_value(*actual, Greek::gamma));
                CHECK_FALSE(gamma_only.has(Greek::delta));
            }
            CHECK_FALSE(actual->has(Greek::vega));
            CHECK_FALSE(actual->has(Greek::theta));
            CHECK(actual->price() == *engine.price(option, market()));
        });
    }
}

TEST_CASE("Digital CRR converges across odd even and off-strike grids", "[digital-numerical][digital-crr]")
{
    for (const double spot : {95.0, 100.0, 105.0})
        for (const int steps : {255, 256, 4095, 4096})
            digital_options([&](const auto& option, bool asset) {
                CAPTURE(spot, steps, asset, option.option_type());
                const auto actual = make_cox_ross_rubinstein_digital_engine({steps})->price_with_greeks(option, market(spot), true);
                const auto expected = AnalyticDigitalEngine{}.price_with_greeks(option, market(spot), {Greek::delta, Greek::gamma});
                REQUIRE(actual);
                REQUIRE(expected);
                const double scale = asset ? spot : 10.0;
                const double error = scale / std::sqrt(steps);
                CHECK(actual->price() == Catch::Approx(expected->price()).margin(error));
                CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(greek_value(*expected, Greek::delta)).margin(error / spot));
                CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(greek_value(*expected, Greek::gamma)).margin(error / spot / spot / 0.04));
            });
}

TEST_CASE("Digital numerical engines retain negative rates dividends and carry", "[digital-numerical]")
{
    const auto mc = *make_monte_carlo_digital_engine({.path_count = 65'536, .seed = 42, .sampling = MonteCarloSampling::sobol});
    const auto tree = *make_cox_ross_rubinstein_digital_engine({4095});
    for (const auto parameters : {std::array{-0.03, 0.01, 0.45}, std::array{0.01, 0.07, 0.3}, std::array{-0.02, -0.05, 0.15}}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(parameters[0], parameters[1], parameters[2]), 100.0, start);
        digital_options([&](const auto& option, bool asset) {
            CAPTURE(parameters, asset, option.option_type());
            const auto expected = AnalyticDigitalEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma}).value();
            const auto check = [&](const auto& engine, bool binomial) {
                const auto actual = engine.price_with_greeks(option, context, true);
                REQUIRE(actual);
                const double error = (asset ? 100.0 : 10.0) * (binomial ? 1.0 / std::sqrt(4095.0) : 1e-3);
                CHECK(actual->price() == Catch::Approx(expected.price()).margin(error));
                CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(greek_value(expected, Greek::delta)).margin(error / 100.0 / parameters[2]));
                CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(greek_value(expected, Greek::gamma)).margin(error / 10'000.0 / parameters[2] / parameters[2]));
            };
            check(mc, false);
            check(tree, true);
        });
    }
}

TEST_CASE("Digital CRR deterministic endpoints retain discounted cash and asset amounts", "[digital-numerical][digital-crr]")
{
    const auto engine = *make_cox_ross_rubinstein_digital_engine({256});
    for (const double rate : {-3.2, 3.2}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.0, 0.2), 100.0, start);
        digital_options([&](const auto& option, bool asset) {
            const bool pays = (option.option_type() == OptionType::call) == (rate > 0.0);
            const auto actual = engine.price(option, context);
            REQUIRE(actual);
            const double expected = pays ? (asset ? 100.0 : 10.0 * std::exp(-rate)) : 0.0;
            CHECK(*actual == Catch::Approx(expected).margin(1e-9));
        });
    }
}

TEST_CASE("Digital numerical engines validate requests lifetimes and configuration", "[digital-numerical]")
{
    CHECK(MonteCarloDigitalEngine{}.settings().path_count == MonteCarloSettings{}.path_count);
    CHECK(MonteCarloDigitalEngine{}.settings().step_count == MonteCarloSettings{}.step_count);
    CHECK(MonteCarloDigitalEngine{}.settings().seed == MonteCarloSettings{}.seed);
    CHECK(CoxRossRubinsteinDigitalEngine{}.settings().step_count == BinomialSettings{}.step_count);
    for (const int count : {0, -1, 1'000'001}) {
        const auto invalid = make_cox_ross_rubinstein_digital_engine({count});
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    for (const auto settings : {MonteCarloSettings{.path_count = 0}, MonteCarloSettings{.path_count = 10'000'001},
                                MonteCarloSettings{.step_count = 1}, MonteCarloSettings{.thread_count = -1},
                                MonteCarloSettings{.path_count = 3, .sampling = MonteCarloSampling::sobol}}) {
        const auto invalid = make_monte_carlo_digital_engine(settings);
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    const auto unsupported = make_monte_carlo_digital_engine({.path_count = 1024, .backend = MonteCarloBackend::cuda, .sampling = MonteCarloSampling::sobol});
    REQUIRE_FALSE(unsupported);
    CHECK(unsupported.error().category == ErrorCategory::unsupported_operation);
    digital_options([&](const auto& option, bool) {
        const auto check = [&](const auto& engine) {
            for (const auto request : {GreeksRequest{}, GreeksRequest{static_cast<Greek>(255)}}) {
                const auto invalid = engine.price_with_greeks(option, market(), request);
                REQUIRE_FALSE(invalid);
                CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
            }
            const auto invalid_shift = engine.price_with_greeks(option, market(), {Greek::rho}, {.spot_shift = 0.0});
            REQUIRE_FALSE(invalid_shift);
            CHECK(invalid_shift.error().category == ErrorCategory::invalid_parameter);
            for (const auto valuation : {start - std::chrono::days{1}, end + std::chrono::days{1}}) {
                const auto invalid_life = engine.price(option, market(100.0, 0.2, start_of_day(valuation)));
                REQUIRE_FALSE(invalid_life);
                CHECK(invalid_life.error().category == ErrorCategory::invalid_time_range);
            }
        };
        check(*make_monte_carlo_digital_engine({.path_count = 3, .seed = 42}));
        check(CoxRossRubinsteinDigitalEngine{});
    });
    const auto unstable = CoxRossRubinsteinDigitalEngine{}.price(
        *make_cash_or_nothing_option(OptionType::call, 100.0, 10.0, start, end), market(100.0, 1e-200));
    REQUIRE_FALSE(unstable);
    CHECK(unstable.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Digital MC preserves tiny-volatility exercise and separates Greek overflow", "[digital-numerical][digital-mc]")
{
    const auto engine = *make_monte_carlo_digital_engine({.path_count = 2, .seed = 42, .antithetic = true});
    for (const double sigma : {1e-200, std::numeric_limits<double>::denorm_min()}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), 100.0,
                                                   start_of_day(end) - std::chrono::microseconds{1});
        digital_options([&](const auto& option, bool asset) {
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price == (asset ? 50.0 : 5.0));
            const auto unrequested = engine.price_with_greeks(option, context, {Greek::rho});
            REQUIRE(unrequested);
            CHECK(unrequested->price() == *price);
            CHECK_FALSE(unrequested->has(Greek::rho));
            const auto overflow = engine.price_with_greeks(option, context, {Greek::gamma});
            REQUIRE_FALSE(overflow);
            CHECK(overflow.error().category == ErrorCategory::invalid_result);
        });
    }
}

TEST_CASE("Digital numerical implied volatility uses brackets tree bounds and one MC seed", "[digital-numerical][digital-implied]")
{
    const auto option = *make_cash_or_nothing_option(OptionType::call, 100.0, 10.0, start, end);
    const auto context = market();
    const auto check = [&](const auto& engine, double tolerance) {
        const double quote = engine.price(option, context).value();
        const auto solved = implied_volatility(engine, option, context, quote, {.lower_bound = 0.1, .upper_bound = 0.3});
        REQUIRE(solved);
        CHECK(*solved == Catch::Approx(0.2).margin(tolerance));
        const auto impossible = implied_volatility(engine, option, context, 20.0, {.lower_bound = 0.1, .upper_bound = 0.3});
        REQUIRE_FALSE(impossible);
        CHECK(impossible.error().category == ErrorCategory::unbracketed_volatility);
    };
    check(*make_cox_ross_rubinstein_digital_engine({1}), 1e-6);
    check(*make_monte_carlo_digital_engine({.path_count = 65'536, .seed = 42}), 1e-3);
    const auto excluded = implied_volatility(*make_cox_ross_rubinstein_digital_engine({1}), option, context, 1.0,
                                             {.lower_bound = 0.001, .upper_bound = 0.02});
    REQUIRE_FALSE(excluded);
    CHECK(excluded.error().category == ErrorCategory::unbracketed_volatility);
    const auto unseeded = *make_monte_carlo_digital_engine({.path_count = 256});
    const auto seeded = detail::with_monte_carlo_seed<MonteCarloDigitalEngine, CashOrNothingOption>(unseeded, [&](const auto& fixed) {
        CHECK(fixed.settings().seed.has_value());
        CHECK(fixed.price(option, context) == fixed.price(option, context));
        return fixed.price(option, context);
    });
    REQUIRE(seeded);
    CHECK_FALSE(unseeded.settings().seed.has_value());
}

TEST_CASE("Digital CUDA prices and likelihood Greeks agree with analytic values", "[digital-numerical][digital-cuda]")
{
    const auto engine = *make_monte_carlo_digital_engine({.path_count = 262'144, .seed = 42, .backend = MonteCarloBackend::cuda});
    digital_options([&](const auto& option, bool asset) {
        const auto actual = engine.price_with_greeks(option, market(), true);
#if KIYOSI_HAS_CUDA
        REQUIRE(actual);
        const auto expected = AnalyticDigitalEngine{}.price_with_greeks(option, market(), {Greek::delta, Greek::gamma}).value();
        const double scale = asset ? 10.0 : 1.0;
        CHECK(actual->price() == Catch::Approx(expected.price()).margin(0.08 * scale));
        CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(greek_value(expected, Greek::delta)).margin(0.003 * scale));
        CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(greek_value(expected, Greek::gamma)).margin(0.0005 * scale));
        CHECK(actual->price() == *engine.price(option, market()));
        const auto gamma_only = engine.price_with_greeks(option, market(), {Greek::gamma}).value();
        CHECK(greek_value(gamma_only, Greek::gamma) == greek_value(*actual, Greek::gamma));
        CHECK_FALSE(gamma_only.has(Greek::delta));
        auto concurrent = std::async(std::launch::async, [&] { return engine.price(option, market()); });
        CHECK(*engine.price(option, market()) == actual->price());
        CHECK(*concurrent.get() == actual->price());
        const auto paired = *make_monte_carlo_digital_engine({.path_count = 2, .seed = 42, .backend = MonteCarloBackend::cuda, .antithetic = true});
        const auto tiny = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, std::numeric_limits<double>::denorm_min()),
                                                100.0, start_of_day(end) - std::chrono::microseconds{1});
        const auto tiny_price = paired.price_with_greeks(option, tiny, {Greek::rho});
        REQUIRE(tiny_price);
        CHECK(tiny_price->price() == (asset ? 50.0 : 5.0));
        CHECK_FALSE(tiny_price->has(Greek::rho));
        const auto overflow = paired.price_with_greeks(option, tiny, {Greek::gamma});
        REQUIRE_FALSE(overflow);
        CHECK(overflow.error().category == ErrorCategory::invalid_result);
#else
        static_cast<void>(asset);
        REQUIRE_FALSE(actual);
        CHECK(actual.error().category == ErrorCategory::backend_unavailable);
#endif
        const auto settled = engine.price_with_greeks(option, market(101.0, 0.2, start_of_day(end)), true);
        REQUIRE(settled);
        CHECK(settled->price() == (option.option_type() == OptionType::call ? (asset ? 101.0 : 10.0) : 0.0));
        CHECK_FALSE(settled->has(Greek::delta));
        CHECK_FALSE(settled->has(Greek::gamma));
    });
}
