#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <cmath>
#include <random>

#include <kiyosi/kiyosi.hpp>

#include "pricing/engines/monte_carlo_host.hpp"
#include "support/common.hpp"

using namespace kiyosi;

TEST_CASE("Antithetic settings default to disabled", "[antithetic]")
{
    CHECK_FALSE(MonteCarloSettings{}.antithetic);
    CHECK_FALSE(TradingDayMonteCarloSettings{}.antithetic);
}

TEST_CASE("Antithetic Sobol products match explicit two-path payoffs", "[antithetic][sobol]")
{
    const auto start = test::day(2025, 1, 1), end = test::day(2025, 1, 2);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.0, 0.2), 100.0, start, all_days_calendar());
    constexpr double dt = 1.0 / 365.0;
    // Independent first-normal reference also used by the Python scenario.
    constexpr double z = -0.10364940758940244;
    const double positive = 100.0 * std::exp(0.03 * dt + 0.2 * std::sqrt(dt) * z);
    const double negative = 100.0 * std::exp(0.03 * dt - 0.2 * std::sqrt(dt) * z);
    const double discount = std::exp(-0.05 * dt);
    const bool antithetic = GENERATE(false, true);
    const int paths = antithetic ? GENERATE(1, 2) : 1;
    const auto average = [&](double first, double second) { return discount * (paths == 1 ? first : (first + second) / 2.0); };
    const MonteCarloSettings vanilla{paths, 3, 42, MonteCarloBackend::cpu, 1, MonteCarloSampling::sobol, antithetic};
    const TradingDayMonteCarloSettings trading{paths, 42, MonteCarloBackend::cpu, 1, MonteCarloSampling::sobol, antithetic};
    const auto check = [&](const auto& engine, const auto& option, double expected) {
        const auto price = engine.price(option, context);
        REQUIRE(price);
        CHECK_THAT(*price, Catch::Matchers::WithinAbs(expected, 1e-11));
        const auto greeks = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(greeks);
        CHECK_THAT(greeks->price(), Catch::Matchers::WithinAbs(expected, 1e-11));
    };
    const auto engine = make_monte_carlo_vanilla_engine(vanilla).value();
    check(engine, *make_european_option(OptionType::call, 90.0, start, end), average(positive - 90.0, negative - 90.0));
    check(engine, *make_american_option(OptionType::call, 100.0, start, end), average(std::max(positive - 100.0, 0.0), std::max(negative - 100.0, 0.0)));
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 100.05, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    check(make_monte_carlo_accumulator_engine(trading).value(), accumulator, average(3.0 * (positive - 100.0), negative - 100.0));
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.06, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.08}}});
    check(make_monte_carlo_autocallable_engine(trading).value(), note, average(1.0 + 0.06 * dt, 1.0 + 0.08 * dt));
}

TEST_CASE("Trading-day antithetic samples preserve exact counts and independent early exits", "[antithetic]")
{
    const int paths = GENERATE(1, 2, 3, 5, 257, 513);
    const bool antithetic = GENERATE(false, true);
    const TradingDayMonteCarloSettings settings{paths, 42, MonteCarloBackend::cpu, 4, MonteCarloSampling::pseudo_random, antithetic};
    double sum = 0.0;
    for (int path = 0; path < paths; ++path) {
        const int samples = antithetic ? (paths + 1) / 2 : paths;
        const bool opposite = antithetic && path >= samples;
        detail::Pcg32 generator{42, static_cast<std::uint64_t>(opposite ? path - samples : path)};
        std::normal_distribution<double> normal;
        const double sign = opposite ? -1.0 : 1.0;
        const double first = sign * normal(generator);
        sum += first > 0.0 ? first : first + sign * normal(generator);
    }
    const auto actual = detail::cpu_path_means<1>(settings, [](auto& generator) -> Result<std::array<double, 1>> {
        std::normal_distribution<double> normal;
        const double first = detail::draw_monte_carlo_normal(generator, normal);
        return std::array{first > 0.0 ? first : first + detail::draw_monte_carlo_normal(generator, normal)}; }, 2);
    REQUIRE(actual);
    CHECK_THAT((*actual)[0], Catch::Matchers::WithinAbs(sum / paths, 1e-13));
}

TEST_CASE("Vanilla Monte Carlo preserves exact odd path payoffs", "[antithetic]")
{
    const int paths = GENERATE(1, 2, 3, 5, 257);
    const bool antithetic = GENERATE(false, true);
    detail::Pcg32 generator{42, 0};
    std::normal_distribution<double> normal;
    double total = 0.0;
    const int samples = antithetic ? (paths + 1) / 2 : paths;
    for (int path = 0; path < samples; ++path) {
        if (path == detail::monte_carlo_block_size) {
            generator = detail::Pcg32{42, 1};
            normal.reset();
        }
        const double z = normal(generator);
        total += std::max(100.0 * std::exp(0.01 + 0.2 * z) - 100.0, 0.0);
        if (antithetic && path < paths / 2)
            total += std::max(100.0 * std::exp(0.01 - 0.2 * z) - 100.0, 0.0);
    }
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const auto engine = make_monte_carlo_vanilla_engine({paths, 3, 42, MonteCarloBackend::cpu, 4, MonteCarloSampling::pseudo_random, antithetic}).value();
    const auto actual = engine.price(option, context);
    REQUIRE(actual);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(std::exp(-0.05) * total / paths, 1e-12));
    const auto cached = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
    REQUIRE(cached);
    CHECK(cached->price() == *actual);
}

TEST_CASE("Antithetic companion failures propagate without changing settings", "[antithetic]")
{
    const auto sampling = GENERATE(MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol);
    const TradingDayMonteCarloSettings settings{2, 42, MonteCarloBackend::cpu, 1, sampling, true};
    int calls = 0;
    const auto failed = detail::cpu_path_means<1>(settings, [&](auto&) -> Result<std::array<double, 1>> {
        if (++calls == 2)
            return std::unexpected(Error{ErrorCategory::invalid_result, "companion failed"});
        return std::array{10.0};
    });
    REQUIRE_FALSE(failed);
    CHECK(failed.error().category == ErrorCategory::invalid_result);
    CHECK(calls == 2);
    const auto recovered = detail::cpu_path_means<1>(settings, [](auto&) -> Result<std::array<double, 1>> { return std::array{10.0}; });
    REQUIRE(recovered);
    CHECK((*recovered)[0] == 10.0);
    CHECK(settings.antithetic);
    CHECK(settings.path_count == 2);
}
