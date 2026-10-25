#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <cmath>
#include <future>
#include <limits>
#include <numbers>
#include <random>

#include <kiyosi/kiyosi.hpp>
#include "pricing/engines/monte_carlo_random.hpp"
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using test::day;
using test::greek_value;
constexpr auto start = day(2025, 1, 1), end = day(2026, 1, 1);

PricingContext market(double spot = 100.0, double sigma = 0.2, Timestamp valuation = start_of_day(start),
                      double rate = 0.04, double dividend = 0.01)
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): the context owns its calendar.
    return make_pricing_context(*make_bsm_parameters(rate, dividend, sigma), spot, valuation).value();
}

template <typename Check>
void asian_options(const Check& check, Date averaging = start, double realized = 0.0, double strike = 100.0)
{
    for (const auto type : {OptionType::call, OptionType::put}) {
        check(*make_arithmetic_average_option(type, strike, start, averaging, end, realized));
        check(*make_geometric_average_option(type, strike, start, averaging, end, realized));
    }
}
} // namespace

TEST_CASE("Asian MC settings and Greek requests preserve core validation", "[asian-mc]")
{
    const MonteCarloSettings defaults;
    const MonteCarloAsianEngine engine;
    CHECK(engine.settings().path_count == defaults.path_count);
    CHECK(engine.settings().step_count == defaults.step_count);
    CHECK(engine.settings().seed == defaults.seed);
    CHECK(engine.settings().backend == defaults.backend);
    CHECK(engine.settings().sampling == defaults.sampling);
    CHECK(engine.settings().thread_count == defaults.thread_count);
    CHECK(engine.settings().antithetic == defaults.antithetic);
    for (const int paths : {0, -1, 10'000'001}) {
        const auto invalid = make_monte_carlo_asian_engine({.path_count = paths});
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    for (const int paths : {1, 10'000'000})
        CHECK(make_monte_carlo_asian_engine({.path_count = paths}));
    for (const int steps : {1, 10'001})
        CHECK_FALSE(make_monte_carlo_asian_engine({.step_count = steps}));
    CHECK_FALSE(make_monte_carlo_asian_engine({.thread_count = -1}));
    CHECK_FALSE(make_monte_carlo_asian_engine({.backend = static_cast<MonteCarloBackend>(9)}));
    CHECK_FALSE(make_monte_carlo_asian_engine({.sampling = static_cast<MonteCarloSampling>(9)}));
    CHECK_FALSE(make_monte_carlo_asian_engine({.sampling = MonteCarloSampling::sobol}));
    CHECK_FALSE(make_monte_carlo_asian_engine({.backend = MonteCarloBackend::cuda, .thread_count = 1}));
    const auto unsupported = make_monte_carlo_asian_engine({.path_count = 64, .backend = MonteCarloBackend::cuda, .sampling = MonteCarloSampling::sobol});
    REQUIRE_FALSE(unsupported);
    CHECK(unsupported.error().category == ErrorCategory::unsupported_operation);
    asian_options([&](const auto& option) {
        const auto bad_request = engine.price_with_greeks(option, market(), {});
        REQUIRE_FALSE(bad_request);
        CHECK(bad_request.error().category == ErrorCategory::invalid_parameter);
        CHECK_FALSE(engine.price_with_greeks(option, market(), {Greek::delta}, {.spot_shift = 0.0}));
        const auto overflow = engine.price(option, market(100.0, std::numeric_limits<double>::max()));
        REQUIRE_FALSE(overflow);
        CHECK(overflow.error().category == ErrorCategory::invalid_result);
    });
}

TEST_CASE("Asian MC history, life boundaries and expiry match existing contracts", "[asian-mc]")
{
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 64, .seed = 7});
    const auto averaging = start + std::chrono::days{90};
    for (const auto valuation : {start_of_day(averaging) - std::chrono::seconds{1}, start_of_day(averaging),
                                 start_of_day(averaging) + std::chrono::seconds{1}}) {
        for (const double realized : {0.0, 100.0}) {
            asian_options([&](const auto& option) {
                const auto priced = engine.price(option, market(100.0, 0.2, valuation));
                const bool valid = valuation > start_of_day(averaging) ? realized > 0.0 : realized == 0.0;
                CHECK(priced.has_value() == valid);
                if (!priced) CHECK(priced.error().category == ErrorCategory::invalid_parameter);
            },
                          averaging, realized);
        }
    }
    asian_options([&](const auto& option) {
        for (const auto valuation : {start_of_day(start) - std::chrono::seconds{1}, start_of_day(end) + std::chrono::seconds{1}}) {
            const auto priced = engine.price(option, market(100.0, 0.2, valuation));
            REQUIRE_FALSE(priced);
            CHECK(priced.error().category == ErrorCategory::invalid_time_range);
        }
    });
    for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
        const auto settled = *make_monte_carlo_asian_engine({.path_count = 1, .seed = 42, .backend = backend});
        asian_options([&](const auto& option) {
            const auto result = settled.price_with_greeks(option, market(50.0, 0.2, start_of_day(end)), true);
            REQUIRE(result);
            CHECK(result->price() == (option.option_type() == OptionType::call ? 10.0 : 0.0));
            for (const auto& greek : result->values_view())
                CHECK_FALSE(greek);
        },
                      start, 110.0);
        asian_options([&](const auto& option) {
            const auto result = settled.price(option, market(110.0, 0.2, start_of_day(end)));
            REQUIRE(result);
            CHECK(*result == (option.option_type() == OptionType::call ? 10.0 : 0.0));
        },
                      end);
    }
}

TEST_CASE("Asian MC geometric price and Greeks converge to independent analytic values", "[asian-mc]")
{
    for (const auto seed : {7, 42, 103})
        for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
            const auto engine = *make_monte_carlo_asian_engine({.path_count = 65'536, .seed = static_cast<std::uint64_t>(seed), .sampling = sampling, .antithetic = true});
            for (const auto type : {OptionType::call, OptionType::put})
                for (const int window : {0, 1, 2}) {
                    const auto averaging = window == 1 ? start + std::chrono::days{120} : start;
                    const auto valuation = window == 2 ? start_of_day(start + std::chrono::days{180}) + std::chrono::hours{12} : start_of_day(start);
                    const auto option = *make_geometric_average_option(type, 100.0, start, averaging, end, window == 2 ? 105.0 : 0.0);
                    const auto context = market(100.0, 0.2, valuation, -0.02);
                    const auto actual = engine.price_with_greeks(option, context, true);
                    const auto expected = AnalyticGeometricAveragePriceEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                    CAPTURE(seed, sampling, window, type);
                    REQUIRE(actual);
                    REQUIRE(expected);
                    CHECK(actual->price() == Catch::Approx(expected->price()).margin(0.08));
                    CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(greek_value(*expected, Greek::delta)).margin(0.009));
                    CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(greek_value(*expected, Greek::gamma)).margin(0.0015));
                    CHECK_FALSE(actual->has(Greek::vega));
                    CHECK_FALSE(actual->has(Greek::speed));
                    CHECK(actual->price() == *engine.price(option, context));
                }
        }
    // Independent 80-digit reference in the existing Asian reference fixture.
    const auto option = *make_geometric_average_option(OptionType::call, 100.0, start, start, end);
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 65'536, .seed = 42, .sampling = MonteCarloSampling::sobol});
    CHECK(*engine.price(option, market()) == Catch::Approx(5.035867546515384).margin(0.002));
}

TEST_CASE("Asian MC arithmetic prices refine to published continuous references", "[asian-mc]")
{
    // QuantLib testVecerEngine: ACT/360 with 360-day years; 365 ACT/365 days gives the same time.
    // https://github.com/lballabio/QuantLib/blob/master/test-suite/asianoptions.cpp
    struct Reference {
        double spot;
        double rate;
        double sigma;
        double price;
    };
    for (const auto test : {Reference{1.9, 0.05, 0.5, 0.193174}, Reference{2.0, 0.05, 0.5, 0.246416},
                            Reference{2.1, 0.05, 0.5, 0.306220}, Reference{2.0, 0.02, 0.1, 0.055986}}) {
        const auto context = market(test.spot, test.sigma, start_of_day(start), test.rate, 0.0);
        const auto call = *make_arithmetic_average_option(OptionType::call, 2.0, start, start, end);
        const auto put = *make_arithmetic_average_option(OptionType::put, 2.0, start, start, end);
        for (const auto seed : {7, 42, 103}) {
            const auto coarse = *make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = 2, .seed = static_cast<std::uint64_t>(seed), .sampling = MonteCarloSampling::sobol, .antithetic = true});
            const auto fine = *make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = 64, .seed = static_cast<std::uint64_t>(seed), .sampling = MonteCarloSampling::sobol, .antithetic = true});
            const double actual = *fine.price(call, context);
            CAPTURE(test.spot, test.sigma, seed);
            CHECK(actual == Catch::Approx(test.price).margin(0.001));
            CHECK(std::abs(actual - test.price) < std::abs(*coarse.price(call, context) - test.price));
            const double parity = std::exp(-test.rate) * (test.spot * std::expm1(test.rate) / test.rate - 2.0);
            CHECK(actual - *fine.price(put, context) == Catch::Approx(parity).margin(0.001));
            const auto pseudo = *make_monte_carlo_asian_engine({.path_count = 1'048'576, .step_count = 64, .seed = static_cast<std::uint64_t>(seed), .antithetic = true});
            // Six conservative sampling errors from independent raw-path moments, plus grid/reference error.
            CHECK(*pseudo.price(call, context) == Catch::Approx(test.price).margin(0.0001 + 0.0048 * test.sigma));
        }
    }
}

TEST_CASE("Asian MC arithmetic conditional Greeks differentiate every future spot contribution", "[asian-mc]")
{
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = 32, .seed = 42, .sampling = MonteCarloSampling::sobol, .antithetic = true});
    for (const auto type : {OptionType::call, OptionType::put})
        for (const int window : {0, 1, 2}) {
            const auto averaging = window == 1 ? start + std::chrono::days{120} : start;
            const auto valuation = window == 2 ? start_of_day(start + std::chrono::days{180}) : start_of_day(start);
            const auto option = *make_arithmetic_average_option(type, 100.0, start, averaging, end, window == 2 ? 110.0 : 0.0);
            const auto context = market(100.0, 0.2, valuation);
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == *engine.price(option, context));
            // Two independently chosen bump widths check derivative and stencil convergence.
            for (const double h : {0.05, 0.025}) {
                const double up = *engine.price(option, market(100.0 + h, 0.2, valuation));
                const double down = *engine.price(option, market(100.0 - h, 0.2, valuation));
                CHECK(greek_value(*result, Greek::delta) == Catch::Approx((up - down) / (2.0 * h)).margin(0.00002));
                CHECK(greek_value(*result, Greek::gamma) == Catch::Approx((up - 2.0 * result->price() + down) / (h * h)).margin(0.00002));
            }
        }
    // Published neighboring continuous price values also bound the continuous sensitivities.
    const auto option = *make_arithmetic_average_option(OptionType::call, 2.0, start, start, end);
    const auto result = *engine.price_with_greeks(option, market(2.0, 0.5, start_of_day(start), 0.05, 0.0), true);
    CHECK(greek_value(result, Greek::delta) == Catch::Approx((0.306220 - 0.193174) / 0.2).margin(0.004));
    CHECK(greek_value(result, Greek::gamma) == Catch::Approx((0.306220 - 2.0 * 0.246416 + 0.193174) / 0.01).margin(0.015));
}

TEST_CASE("Asian MC arithmetic conditioning agrees with independent raw-path sensitivities", "[asian-mc]")
{
    constexpr int paths = 131'072, steps = 32;
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = steps, .seed = 42, .sampling = MonteCarloSampling::sobol, .antithetic = true});
    for (const int window : {0, 1, 2}) {
        const double lead = window == 1 ? 120.0 / 365.0 : 0.0;
        const double future = window == 2 ? 185.0 / 365.0 : 1.0 - lead;
        const double weight = window == 2 ? future : 1.0;
        const double history = window == 2 ? (1.0 - weight) * 105.0 : 0.0;
        const double dt = future / steps, discount = std::exp(-0.04 * (lead + future));
        std::mt19937_64 generator{1977}; // NOLINT(bugprone-random-generator-seed): reproducible independent numerical reference.
        std::normal_distribution<double> normal;
        std::array<double, 4> sums{}, squares{};
        for (int path = 0; path < paths; ++path) {
            double asset = 100.0 * std::exp(0.01 * lead + 0.2 * std::sqrt(lead) * normal(generator));
            double integral = 0.5 * asset;
            for (int step = 1; step <= steps; ++step) {
                asset *= std::exp(0.01 * dt + 0.2 * std::sqrt(dt) * normal(generator));
                integral += (step == steps ? 0.5 : 1.0) * asset;
            }
            const double average = weight * integral / steps;
            const auto payoff = [&](double spot) { return discount * std::max(history + average * (spot / 100.0) - 100.0, 0.0); };
            const double price = payoff(100.0);
            const double delta = history + average > 100.0 ? discount * average / 100.0 : 0.0;
            const std::array values{price, delta, (payoff(100.5) - 2.0 * price + payoff(99.5)) / 0.25,
                                    (payoff(100.25) - 2.0 * price + payoff(99.75)) / 0.0625};
            for (std::size_t i = 0; i < values.size(); ++i) {
                sums[i] += values[i];
                squares[i] += values[i] * values[i];
            }
        }
        const auto option = *make_arithmetic_average_option(OptionType::call, 100.0, start,
                                                            window == 1 ? start + std::chrono::days{120} : start, end, window == 2 ? 105.0 : 0.0);
        const auto context = market(100.0, 0.2, window == 2 ? start_of_day(start + std::chrono::days{180}) : start_of_day(start));
        const auto actual = *engine.price_with_greeks(option, context, true);
        const std::array actuals{actual.price(), greek_value(actual, Greek::delta), greek_value(actual, Greek::gamma), greek_value(actual, Greek::gamma)};
        for (std::size_t i = 0; i < sums.size(); ++i) {
            const double mean = sums[i] / paths;
            const double standard_error = std::sqrt((squares[i] / paths - mean * mean) / (paths - 1));
            CAPTURE(window, i, standard_error);
            CHECK(actuals[i] == Catch::Approx(mean).margin(6.0 * standard_error + 0.00005));
        }
    }
}

TEST_CASE("Asian MC conditional affine branches and monetary scaling retain valid prices", "[asian-mc]")
{
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = 2, .seed = 42, .sampling = MonteCarloSampling::sobol});
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_arithmetic_average_option(type, 20.0, start, start, end);
        const auto result = *engine.price_with_greeks(option, market(), true);
        const double mean = 100.0 * (0.25 + 0.5 * std::exp(0.015) + 0.25 * std::exp(0.03));
        CHECK(result.price() == Catch::Approx(type == OptionType::call ? std::exp(-0.04) * (mean - 20.0) : 0.0).margin(0.002));
        CHECK(greek_value(result, Greek::gamma) == 0.0);
        CHECK(greek_value(result, Greek::delta) == Catch::Approx(type == OptionType::call ? std::exp(-0.04) * mean / 100.0 : 0.0).margin(0.00002));
    }
    for (const double scale : {1e-200, 1.0, 1e300}) {
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto a = *make_arithmetic_average_option(type, 100.0, start, start, end);
            const auto g = *make_geometric_average_option(type, 100.0, start, start, end);
            const auto scaled_a = *make_arithmetic_average_option(type, 100.0 * scale, start, start, end);
            const auto scaled_g = *make_geometric_average_option(type, 100.0 * scale, start, start, end);
            CHECK(*engine.price(scaled_a, market(100.0 * scale)) / scale == Catch::Approx(*engine.price(a, market())).epsilon(1e-11));
            CHECK(*engine.price(scaled_g, market(100.0 * scale)) / scale == Catch::Approx(*engine.price(g, market())).epsilon(1e-11));
        }
    }
}

TEST_CASE("Asian MC preserves scaled tiny-volatility prices and seasoned negative gamma", "[asian-mc]")
{
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = 64, .seed = 42, .sampling = MonteCarloSampling::sobol, .antithetic = true});
    for (const auto [spot, sigma] : {std::pair{100.0, 1e-170}, {1e160, 1e-160}, {1e300, 1e-200}, {1e300, std::numeric_limits<double>::denorm_min()}}) {
        const double expected = std::exp(std::log(spot) + std::log(sigma)) / std::sqrt(6.0 * std::numbers::pi);
        asian_options([&](const auto& option) {
            const auto price = engine.price(option, market(spot, sigma, start_of_day(start), 0.0, 0.0));
            REQUIRE(price);
            CHECK(*price == Catch::Approx(expected).epsilon(0.002));
        },
                      start, 0.0, spot);
    }
    const auto option = *make_geometric_average_option(OptionType::call, 10.0, start, start, end, 100.0);
    const auto context = market(100.0, 0.2, start_of_day(start + std::chrono::days{180}));
    const auto result = *engine.price_with_greeks(option, context, true);
    const auto expected = *AnalyticGeometricAveragePriceEngine{}.price_with_greeks(option, context, {Greek::gamma});
    CHECK(greek_value(result, Greek::gamma) < 0.0);
    CHECK(greek_value(result, Greek::gamma) == Catch::Approx(greek_value(expected, Greek::gamma)).margin(0.0001));
    const auto near_strike = *make_geometric_average_option(OptionType::call, 99.9999, start, start, end);
    const auto near_value = engine.price(near_strike, market(100.0, std::numeric_limits<double>::denorm_min(), start_of_day(start), 0.0, 0.0));
    REQUIRE(near_value);
    CHECK(*near_value == Catch::Approx(100.0 - 99.9999).epsilon(1e-9));
}

TEST_CASE("Asian MC affine settlements need no backend and IV preserves domain rejections", "[asian-mc]")
{
    for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
        const auto engine = *make_monte_carlo_asian_engine({.path_count = 1, .seed = 7, .backend = backend});
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto option = *make_arithmetic_average_option(type, 25.0, start, start, end, 100.0);
            const auto context = market(100.0, 0.2, start_of_day(start + std::chrono::days{180}));
            const auto result = engine.price_with_greeks(option, context, true);
            REQUIRE(result);
            CHECK(result->price() == *TurnbullWakemanArithmeticAveragePriceEngine{}.price(option, context));
            CHECK(greek_value(*result, Greek::gamma) == 0.0);
            const double future = 185.0 / 365.0;
            const double expected_delta = type == OptionType::call ? std::exp(-0.04 * future) * std::expm1(0.03 * future) / 0.03 : 0.0;
            CHECK(greek_value(*result, Greek::delta) == Catch::Approx(expected_delta).epsilon(1e-12));
            const auto iv = implied_volatility(engine, option, context, result->price());
            REQUIRE_FALSE(iv);
            CHECK(iv.error().category == ErrorCategory::unsupported_operation);
        }
    }
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 16'384, .step_count = 16, .seed = 42, .sampling = MonteCarloSampling::sobol});
    asian_options([&](const auto& option) {
        const double observed = *engine.price(option, market());
        const auto iv = implied_volatility(engine, option, market(100.0, 0.35), observed);
        REQUIRE(iv);
        CHECK(*iv == Catch::Approx(0.2).margin(0.00001));
    });
    const auto unseeded = *make_monte_carlo_asian_engine({.path_count = 65'536, .sampling = MonteCarloSampling::sobol});
    const auto geometric = *make_geometric_average_option(OptionType::call, 100.0, start, start, end);
    const double observed = *AnalyticGeometricAveragePriceEngine{}.price(geometric, market());
    const auto solved = implied_volatility(unseeded, geometric, market(100.0, 0.35), observed);
    REQUIRE(solved);
    CHECK(*solved == Catch::Approx(0.2).margin(0.002));
    CHECK_FALSE(unseeded.settings().seed);
}

TEST_CASE("Asian MC seeded calls are stable across threads, concurrency and exact distributions", "[asian-mc]")
{
    for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
        const auto serial = *make_monte_carlo_asian_engine({.path_count = 4096, .step_count = 16, .seed = 42, .thread_count = 1, .sampling = sampling, .antithetic = true});
        auto settings = serial.settings();
        settings.thread_count = 4;
        const auto parallel = *make_monte_carlo_asian_engine(settings);
        asian_options([&](const auto& option) {
            const auto expected = *serial.price_with_greeks(option, market(), true);
            auto first = std::async(std::launch::async, [&] { return parallel.price_with_greeks(option, market(), true); });
            const auto second = parallel.price_with_greeks(option, market(), true);
            const auto concurrent = first.get();
            REQUIRE(second);
            REQUIRE(concurrent);
            CHECK(second->price() == expected.price());
            CHECK(concurrent->values_view() == expected.values_view());
            CHECK(second->values_view() == expected.values_view());
        });
        for (const int steps : {2, 10'000}) {
            settings.step_count = steps;
            const auto engine = *make_monte_carlo_asian_engine(settings);
            asian_options([&](const auto& option) {
                const double expected = *serial.price(option, market());
                CHECK(*engine.price(option, market()) == expected);
            },
                          end);
            const auto geometric = *make_geometric_average_option(OptionType::call, 100.0, start, start, end);
            CHECK(*engine.price(geometric, market()) == *serial.price(geometric, market()));
        }
    }
    for (const int paths : {1, 2, 3, 4, 5}) {
        const auto engine = *make_monte_carlo_asian_engine({.path_count = paths, .seed = 42, .antithetic = true});
        const auto option = *make_geometric_average_option(OptionType::call, 100.0, start, end, end);
        double expected = 0.0;
        for (int path = 0; path < paths; ++path) {
            detail::Pcg32 generator{42, static_cast<std::uint64_t>(path / 2)};
            std::normal_distribution<double> normal;
            const double z = normal(generator) * (path % 2 == 0 ? 1.0 : -1.0);
            expected += std::exp(-0.04) * std::max(100.0 * std::exp(0.01 + 0.2 * z) - 100.0, 0.0) / paths;
        }
        CHECK(*engine.price(option, market()) == Catch::Approx(expected).margin(1e-11));
    }
}

TEST_CASE("Asian MC CUDA prices and Greeks preserve CPU contract semantics", "[asian-mc][cuda]")
{
    const auto cpu = *make_monte_carlo_asian_engine({.path_count = 131'072, .step_count = 32, .seed = 42, .antithetic = true});
    auto settings = cpu.settings();
    settings.backend = MonteCarloBackend::cuda;
    const auto gpu = *make_monte_carlo_asian_engine(settings);
    for (const int window : {0, 1, 2}) {
        const auto averaging = window == 1 ? start + std::chrono::days{120} : start;
        const auto valuation = window == 2 ? start_of_day(start + std::chrono::days{180}) : start_of_day(start);
        asian_options([&](const auto& option) {
            const auto context = market(100.0, 0.2, valuation);
            const auto actual = gpu.price_with_greeks(option, context, true);
            if (!actual) {
                CHECK(actual.error().category == ErrorCategory::backend_unavailable);
                return;
            }
            const auto expected = *cpu.price_with_greeks(option, context, true);
            CHECK(actual->price() == Catch::Approx(expected.price()).margin(0.08));
            CHECK(greek_value(*actual, Greek::delta) == Catch::Approx(greek_value(expected, Greek::delta)).margin(0.01));
            CHECK(greek_value(*actual, Greek::gamma) == Catch::Approx(greek_value(expected, Greek::gamma)).margin(0.002));
            CHECK_FALSE(actual->has(Greek::rho));
            CHECK(actual->price() == *gpu.price(option, context));
            const auto only_gamma = gpu.price_with_greeks(option, context, {Greek::gamma});
            REQUIRE(only_gamma);
            CHECK_FALSE(only_gamma->has(Greek::delta));
            CHECK(greek_value(*only_gamma, Greek::gamma) == greek_value(*actual, Greek::gamma));
        },
                      averaging, window == 2 ? 100.0 : 0.0);
    }
    const auto single = *make_geometric_average_option(OptionType::call, 1.0, start, end, end);
    std::array<double, 3> positive{};
    double total = 0.0;
    settings.antithetic = false;
    for (int paths = 1; paths <= 3; ++paths) {
        settings.path_count = paths;
        const auto value = make_monte_carlo_asian_engine(settings)->price(single, market());
        if (!value) {
            CHECK(value.error().category == ErrorCategory::backend_unavailable);
            return;
        }
        const double assets = (*value + std::exp(-0.04)) * paths;
        positive[paths - 1] = assets - total;
        total = assets;
    }
    settings.antithetic = true;
    total = 0.0;
    for (int paths = 1; paths <= 5; ++paths) {
        const int pair = (paths - 1) / 2;
        total += paths % 2 == 1 ? positive[pair] : 10'000.0 * std::exp(-0.06) / positive[pair];
        settings.path_count = paths;
        const auto value = make_monte_carlo_asian_engine(settings)->price(single, market());
        REQUIRE(value);
        CHECK(*value == Catch::Approx(total / paths - std::exp(-0.04)).margin(1e-10));
    }
}
