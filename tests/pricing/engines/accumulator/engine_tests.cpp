#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <atomic>
#include <cmath>
#include <memory>
#include <random>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;

double legacy_accumulator_price(const kiyosi::Accumulator& option,
                                const kiyosi::PricingContext& context,
                                kiyosi::StructuredMonteCarloSettings settings)
{
    const auto path_payoff = [&](std::mt19937_64& generator) {
        const double rate = context.parameters().risk_free_rate();
        const double dividend = context.parameters().dividend_yield();
        const double sigma = context.parameters().volatility();
        const auto valuation = context.valuation_time();

        double value = context.asset_price();
        double quantity = option.accumulated_quantity();
        double terminal = value;
        if (valuation == kiyosi::start_of_day(kiyosi::date_of(valuation)) &&
            context.calendar().is_trading_day(kiyosi::date_of(valuation))) {
            if (value >= option.knock_out()) return quantity * (value - option.strike());
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                                : option.daily_quantity();
        }
        if (valuation == option.expiry()) return quantity * (value - option.strike());

        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (auto current = kiyosi::date_of(valuation) + std::chrono::days{1};
             current <= option.expiry(); current += std::chrono::days{1}) {
            if (!context.calendar().is_trading_day(current)) continue;
            const double dt = *kiyosi::year_fraction(previous, kiyosi::start_of_day(current));
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                              sigma * std::sqrt(dt) * normal(generator));
            previous = kiyosi::start_of_day(current);
            if (value >= option.knock_out()) {
                terminal = value;
                break;
            }
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                                : option.daily_quantity();
            terminal = value;
        }
        return quantity * (terminal - option.strike()) *
               std::exp(-rate * *kiyosi::year_fraction(valuation, previous));
    };

    std::mt19937_64 generator(*settings.seed);
    double sum = 0.0;
    for (int path = 0; path < settings.path_count; ++path)
        sum += path_payoff(generator);
    return sum / static_cast<double>(settings.path_count);
}
}

TEST_CASE("Accumulator expiry settlement agrees across pricing engines")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), spot, expiry);
    };
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective = effective,
                                                        .expiry = expiry});

    const auto monte_carlo = kiyosi::MonteCarloAccumulatorEngine{{32, 7}}.price(accumulator, market(90.0));
    REQUIRE(monte_carlo);
    CHECK(*monte_carlo->require(kiyosi::risk_measure::price) == Catch::Approx(-50.0));

    const auto finite_difference =
        kiyosi::FiniteDifferenceAccumulatorEngine{}.price(accumulator, market(90.0));
    REQUIRE(finite_difference);
    CHECK(*finite_difference->require(kiyosi::risk_measure::price) == Catch::Approx(-50.0));
}

TEST_CASE("Accumulator Monte Carlo prepares stable calendar inputs once")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2025, 1, 6);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::date value) {
            ++*calls;
            return value == valuation || value == day(2025, 1, 2) ||
                   value == day(2025, 1, 4) || value == expiry;
        },
        252);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2), 100.0, valuation, calendar);
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out = 101.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective = valuation,
                                                        .expiry = expiry});
    const kiyosi::StructuredMonteCarloSettings settings{32, 17};
    const double legacy = legacy_accumulator_price(accumulator, context, settings);
    calls->store(0);

    const auto result = kiyosi::MonteCarloAccumulatorEngine{settings}.price(accumulator, context);

    REQUIRE(result);
    CHECK(*result->require(kiyosi::risk_measure::price) == legacy);
    CHECK(calls->load() == 6);
}

TEST_CASE("Accumulator finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration = 2.0,
                                                        .accumulated_quantity = 3.0,
                                                        .effective = effective,
                                                        .expiry = expiry});
    for (const auto scheme : {kiyosi::finite_difference_scheme::explicit_euler,
                              kiyosi::finite_difference_scheme::implicit_euler,
                              kiyosi::finite_difference_scheme::crank_nicolson}) {
        const auto coarse = kiyosi::FiniteDifferenceAccumulatorEngine{{40, 512, scheme}}.price(accumulator, context);
        const auto fine = kiyosi::FiniteDifferenceAccumulatorEngine{{80, 1024, scheme}}.price(accumulator, context);
        REQUIRE(coarse);
        REQUIRE(fine);
        const double coarse_value = *coarse->require(kiyosi::risk_measure::price);
        const double fine_value = *fine->require(kiyosi::risk_measure::price);
        CHECK(std::isfinite(coarse_value));
        CHECK(std::isfinite(fine_value));
        CHECK(fine_value != coarse_value);
    }
}
