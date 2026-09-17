#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <atomic>
#include <chrono>
#include <memory>
#include <random>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;

double legacy_binary_snowball_price(const kiyosi::BinarySnowballOption& note,
                                    const kiyosi::PricingContext& context,
                                    kiyosi::StructuredMonteCarloSettings settings)
{
    const auto path_payoff = [&](std::mt19937_64& generator) {
        if (note.touch_status() == kiyosi::barrier_touch_status::up) return 0.0;

        const double rate = context.parameters().risk_free_rate();
        const double dividend = context.parameters().dividend_yield();
        const double sigma = context.parameters().volatility();
        const auto valuation = context.valuation_time();
        const auto& dates = note.observation_dates();
        std::vector<std::size_t> schedule;
        for (std::size_t index = 0; index < dates.size(); ++index)
            if (dates[index] >= valuation) schedule.push_back(index);

        double value = context.asset_price();
        std::size_t index = 0;
        if (!schedule.empty() && dates[schedule.front()] == valuation) {
            const auto event = schedule.front();
            const double coupon = note.knock_out_coupon_rates()[event] *
                                  *kiyosi::year_fraction(note.effective(), dates[event]);
            if (value >= note.knock_out_prices()[event]) return note.principal_ratio() + coupon;
            index = 1;
        }
        const double terminal = note.principal_ratio() +
                                note.maturity_coupon_rate() *
                                    *kiyosi::year_fraction(note.effective(), note.expiry());
        if (valuation == note.expiry()) return terminal;

        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (auto current = kiyosi::date_of(valuation) + std::chrono::days{1};
             current <= note.expiry(); current += std::chrono::days{1}) {
            if (!context.calendar().is_trading_day(current)) continue;
            const double dt = *kiyosi::year_fraction(previous, kiyosi::start_of_day(current));
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                              sigma * std::sqrt(dt) * normal(generator));
            previous = kiyosi::start_of_day(current);
            if (index >= schedule.size() || dates[schedule[index]] != current) continue;
            const auto event = schedule[index];
            const double coupon = note.knock_out_coupon_rates()[event] *
                                  *kiyosi::year_fraction(note.effective(), dates[event]);
            if (value >= note.knock_out_prices()[event])
                return (note.principal_ratio() + coupon) *
                       std::exp(-rate * *kiyosi::year_fraction(valuation, previous));
            ++index;
        }
        return std::exp(-rate *
                        *kiyosi::year_fraction(valuation, kiyosi::start_of_day(note.expiry()))) *
               terminal;
    };

    std::mt19937_64 generator(*settings.seed);
    double sum = 0.0;
    for (int path = 0; path < settings.path_count; ++path)
        sum += path_payoff(generator);
    return sum / static_cast<double>(settings.path_count);
}

template <typename Instrument>
void check_structured_refinement(const Instrument& instrument, const kiyosi::PricingContext& context)
{
    for (const auto scheme : {kiyosi::finite_difference_scheme::explicit_euler,
                              kiyosi::finite_difference_scheme::implicit_euler,
                              kiyosi::finite_difference_scheme::crank_nicolson}) {
        const auto coarse = kiyosi::FiniteDifferenceStructuredEngine<Instrument>{{40, 512, scheme}}.price(instrument, context);
        const auto fine = kiyosi::FiniteDifferenceStructuredEngine<Instrument>{{80, 1024, scheme}}.price(instrument, context);
        REQUIRE(coarse);
        REQUIRE(fine);
        const double coarse_value = *coarse->require(kiyosi::risk_measure::price);
        const double fine_value = *fine->require(kiyosi::risk_measure::price);
        CHECK(std::isfinite(coarse_value));
        CHECK(std::isfinite(fine_value));
        CHECK(fine_value != coarse_value);
    }
}

TEST_CASE("Phoenix expiry settlement applies state and final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->require(kiyosi::risk_measure::price);
    };

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_price = 100.0,
                                                       .knock_in_price = 80.0,
                                                       .knock_out_prices = {100.0},
                                                       .coupon_barriers = {90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {expiry},
                                                       .frequency = kiyosi::observation_frequency::at_expiry,
                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                       .principal_ratio = 1.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    CHECK(price(phoenix, 100.0) == Catch::Approx(9.0));
    CHECK(price(phoenix, 80.0) == Catch::Approx(1.0));
    const auto phoenix_up = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                          .initial_price = 100.0,
                                                          .knock_in_price = 80.0,
                                                          .knock_out_prices = {100.0},
                                                          .coupon_barriers = {90.0},
                                                          .upper_strike = 100.0,
                                                          .lower_strike = 60.0,
                                                          .observation_dates = {expiry},
                                                          .frequency = kiyosi::observation_frequency::daily,
                                                          .touch_status = kiyosi::barrier_touch_status::up,
                                                          .principal_ratio = 1.0,
                                                          .effective = effective,
                                                          .expiry = expiry});
    CHECK(price(phoenix_up, 100.0) == 0.0);
}

TEST_CASE("Snowball expiry settlement applies state and final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->require(kiyosi::risk_measure::price);
    };
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.10},
                                                         .maturity_coupon_rate = 0.05,
                                                         .initial_price = 100.0,
                                                         .knock_in_price = 80.0,
                                                         .knock_out_prices = {100.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {expiry},
                                                         .frequency = kiyosi::observation_frequency::at_expiry,
                                                         .touch_status = kiyosi::barrier_touch_status::none,
                                                         .principal_ratio = 1.0,
                                                         .effective = effective,
                                                         .expiry = expiry});
    CHECK(price(snowball, 100.0) == Catch::Approx(1.10));
    CHECK(price(snowball, 80.0) == Catch::Approx(1.05));
    CHECK(price(snowball, 59.0) == Catch::Approx(0.60));
    const auto snowball_down = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.10},
                                                              .maturity_coupon_rate = 0.05,
                                                              .initial_price = 100.0,
                                                              .knock_in_price = 80.0,
                                                              .knock_out_prices = {110.0},
                                                              .upper_strike = 100.0,
                                                              .lower_strike = 60.0,
                                                              .observation_dates = {expiry},
                                                              .frequency = kiyosi::observation_frequency::daily,
                                                              .touch_status = kiyosi::barrier_touch_status::down,
                                                              .principal_ratio = 1.0,
                                                              .effective = effective,
                                                              .expiry = expiry});
    CHECK(price(snowball_down, 70.0) == Catch::Approx(0.70));
}

TEST_CASE("Binary snowball expiry settlement applies final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->require(kiyosi::risk_measure::price);
    };
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10},
                                                              .maturity_coupon_rate = 0.05,
                                                              .initial_price = 100.0,
                                                              .knock_out_prices = {100.0},
                                                              .upper_strike = 100.0,
                                                              .lower_strike = 60.0,
                                                              .observation_dates = {expiry},
                                                              .touch_status = kiyosi::barrier_touch_status::none,
                                                              .principal_ratio = 1.0,
                                                              .effective = effective,
                                                              .expiry = expiry});
    CHECK(price(binary, 100.0) == Catch::Approx(1.10));
    CHECK(price(binary, 90.0) == Catch::Approx(1.05));
}

TEST_CASE("Ternary snowball expiry settlement applies final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->require(kiyosi::risk_measure::price);
    };
    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.10},
                                                                .maturity_coupon_rate = 0.05,
                                                                .minimal_coupon_rate = 0.02,
                                                                .initial_price = 100.0,
                                                                .knock_in_price = 80.0,
                                                                .knock_out_prices = {100.0},
                                                                .upper_strike = 100.0,
                                                                .lower_strike = 60.0,
                                                                .observation_dates = {expiry},
                                                                .frequency = kiyosi::observation_frequency::at_expiry,
                                                                .touch_status = kiyosi::barrier_touch_status::none,
                                                                .principal_ratio = 1.0,
                                                                .effective = effective,
                                                                .expiry = expiry});
    CHECK(price(ternary, 100.0) == Catch::Approx(1.10));
    CHECK(price(ternary, 80.0) == Catch::Approx(1.05));
    CHECK(price(ternary, 79.0) == Catch::Approx(1.02));
}

TEST_CASE("Structured Monte Carlo processes valuation-date observation events once")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0, valuation);
    const auto monte_carlo_price = [&](const auto& instrument) {
        using Instrument = std::remove_cvref_t<decltype(instrument)>;
        const auto result = kiyosi::MonteCarloStructuredEngine<Instrument>{{32, 7}}.price(instrument, context);
        REQUIRE(result);
        return *result->require(kiyosi::risk_measure::price);
    };
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {99.0, 10.0, 0.10},
                                                            .maturity_coupon_rate = 0.05,
                                                            .initial_price = 100.0,
                                                            .knock_out_prices = {90.0, 90.0, 99.0},
                                                            .upper_strike = 100.0,
                                                            .lower_strike = 60.0,
                                                            .observation_dates = {effective, valuation, expiry},
                                                            .touch_status = kiyosi::barrier_touch_status::none,
                                                            .principal_ratio = 1.0,
                                                            .effective = effective,
                                                            .expiry = expiry});
    const kiyosi::MonteCarloBinarySnowballEngine engine{{32, 7}};
    const auto first = engine.price(note, context);
    const auto second = engine.price(note, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(*first->require(kiyosi::risk_measure::price) == *second->require(kiyosi::risk_measure::price));
    CHECK(*first->require(kiyosi::risk_measure::price) ==
          Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective, valuation).value()).margin(1e-10));
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {99.0, 10.0, 0.10},
                                                         .maturity_coupon_rate = 0.05,
                                                         .initial_price = 100.0,
                                                         .knock_in_price = 80.0,
                                                         .knock_out_prices = {90.0, 90.0, 99.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {effective, valuation, expiry},
                                                         .frequency = kiyosi::observation_frequency::at_expiry,
                                                         .touch_status = kiyosi::barrier_touch_status::none,
                                                         .principal_ratio = 1.0,
                                                         .effective = effective,
                                                         .expiry = expiry});
    CHECK(monte_carlo_price(snowball) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective, valuation).value()));

    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {99.0, 10.0, 0.10},
                                                                .maturity_coupon_rate = 0.05,
                                                                .minimal_coupon_rate = 0.02,
                                                                .initial_price = 100.0,
                                                                .knock_in_price = 80.0,
                                                                .knock_out_prices = {90.0, 90.0, 99.0},
                                                                .upper_strike = 100.0,
                                                                .lower_strike = 60.0,
                                                                .observation_dates = {effective, valuation, expiry},
                                                                .frequency = kiyosi::observation_frequency::at_expiry,
                                                                .touch_status = kiyosi::barrier_touch_status::none,
                                                                .principal_ratio = 1.0,
                                                                .effective = effective,
                                                                .expiry = expiry});
    CHECK(monte_carlo_price(ternary) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective, valuation).value()));

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_price = 100.0,
                                                       .knock_in_price = 80.0,
                                                       .knock_out_prices = {90.0, 90.0, 99.0},
                                                       .coupon_barriers = {90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {effective, valuation, expiry},
                                                       .frequency = kiyosi::observation_frequency::at_expiry,
                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                       .principal_ratio = 1.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    CHECK(monte_carlo_price(phoenix) == Catch::Approx(9.0));
}

TEST_CASE("Structured Monte Carlo prepares stable calendar inputs once")
{
    const auto valuation = day(2025, 1, 1);
    const auto first_observation = day(2025, 1, 4);
    const auto expiry = day(2025, 1, 6);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::date value) {
            ++*calls;
            return value == valuation || value == day(2025, 1, 2) ||
                   value == first_observation || value == expiry;
        },
        252);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2), 100.0, valuation, calendar);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10, 0.20},
                                                            .maturity_coupon_rate = 0.02,
                                                            .initial_price = 100.0,
                                                            .knock_out_prices = {100.0, 100.0},
                                                            .upper_strike = 100.0,
                                                            .lower_strike = 60.0,
                                                            .observation_dates = {first_observation, expiry},
                                                            .touch_status = kiyosi::barrier_touch_status::none,
                                                            .principal_ratio = 1.0,
                                                            .effective = valuation,
                                                            .expiry = expiry});
    const kiyosi::StructuredMonteCarloSettings settings{32, 17};
    const double legacy = legacy_binary_snowball_price(note, context, settings);
    calls->store(0);

    const auto result = kiyosi::MonteCarloBinarySnowballEngine{settings}.price(note, context);

    REQUIRE(result);
    CHECK(*result->require(kiyosi::risk_measure::price) == legacy);
    CHECK(calls->load() == 7);
}

TEST_CASE("Structured finite difference preserves future observation indices")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation_date = day(2025, 1, 2);
    const auto future_observation = day(2025, 1, 4);
    const auto expiry = day(2025, 1, 6);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {100.0, 0.2, 0.3},
                                                            .maturity_coupon_rate = 0.01,
                                                            .initial_price = 100.0,
                                                            .knock_out_prices = {90.0, 90.0, 90.0},
                                                            .upper_strike = 100.0,
                                                            .lower_strike = 60.0,
                                                            .observation_dates = {valuation_date, future_observation, expiry},
                                                            .touch_status = kiyosi::barrier_touch_status::none,
                                                            .principal_ratio = 1.0,
                                                            .effective = effective,
                                                            .expiry = expiry});
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0,
        kiyosi::start_of_day(valuation_date) + std::chrono::hours{12},
        kiyosi::all_days_calendar());

    const auto result = kiyosi::FiniteDifferenceBinarySnowballEngine{{100, 3}}.price(note, context);

    REQUIRE(result);
    const double expected = 1.0 + 0.2 * *kiyosi::year_fraction(effective, future_observation);
    CHECK(*result->require(kiyosi::risk_measure::price) == Catch::Approx(expected).margin(1e-12));
}

TEST_CASE("Phoenix finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.02,
                                                       .initial_price = 100.0,
                                                       .knock_in_price = 75.0,
                                                       .knock_out_prices = knock_outs,
                                                       .coupon_barriers = {90.0, 90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = observation_dates,
                                                       .frequency = kiyosi::observation_frequency::daily,
                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                       .principal_ratio = 1.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    check_structured_refinement(phoenix, context);
}

TEST_CASE("Snowball finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = coupons,
                                                         .maturity_coupon_rate = 0.08,
                                                         .initial_price = 100.0,
                                                         .knock_in_price = 75.0,
                                                         .knock_out_prices = knock_outs,
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = observation_dates,
                                                         .frequency = kiyosi::observation_frequency::daily,
                                                         .touch_status = kiyosi::barrier_touch_status::none,
                                                         .principal_ratio = 1.0,
                                                         .effective = effective,
                                                         .expiry = expiry});
    check_structured_refinement(snowball, context);
}

TEST_CASE("Binary snowball finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = coupons,
                                                              .maturity_coupon_rate = 0.08,
                                                              .initial_price = 100.0,
                                                              .knock_out_prices = knock_outs,
                                                              .upper_strike = 100.0,
                                                              .lower_strike = 60.0,
                                                              .observation_dates = observation_dates,
                                                              .touch_status = kiyosi::barrier_touch_status::none,
                                                              .principal_ratio = 1.0,
                                                              .effective = effective,
                                                              .expiry = expiry});
    check_structured_refinement(binary, context);
}

TEST_CASE("Ternary snowball finite-difference engine refines its event-aware BSM grid")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = coupons,
                                                                .maturity_coupon_rate = 0.08,
                                                                .minimal_coupon_rate = 0.02,
                                                                .initial_price = 100.0,
                                                                .knock_in_price = 75.0,
                                                                .knock_out_prices = knock_outs,
                                                                .upper_strike = 100.0,
                                                                .lower_strike = 60.0,
                                                                .observation_dates = observation_dates,
                                                                .frequency = kiyosi::observation_frequency::daily,
                                                                .touch_status = kiyosi::barrier_touch_status::none,
                                                                .principal_ratio = 1.0,
                                                                .effective = effective,
                                                                .expiry = expiry});
    check_structured_refinement(ternary, context);
}

TEST_CASE("Finite-difference binary snowball engine rejects unstable explicit grids")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = coupons,
                                                              .maturity_coupon_rate = 0.08,
                                                              .initial_price = 100.0,
                                                              .knock_out_prices = knock_outs,
                                                              .upper_strike = 100.0,
                                                              .lower_strike = 60.0,
                                                              .observation_dates = observation_dates,
                                                              .touch_status = kiyosi::barrier_touch_status::none,
                                                              .principal_ratio = 1.0,
                                                              .effective = effective,
                                                              .expiry = expiry});
    CHECK_FALSE(kiyosi::FiniteDifferenceBinarySnowballEngine{{40, 1, kiyosi::finite_difference_scheme::explicit_euler}}
                    .price(binary, context));
}

TEST_CASE("Finite-difference phoenix engine rejects domains below the barrier")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                 day(2025, 10, 1), expiry};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.02,
                                                       .initial_price = 100.0,
                                                       .knock_in_price = 75.0,
                                                       .knock_out_prices = knock_outs,
                                                       .coupon_barriers = {90.0, 90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = observation_dates,
                                                       .frequency = kiyosi::observation_frequency::daily,
                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                       .principal_ratio = 1.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    CHECK_FALSE(kiyosi::FiniteDifferencePhoenixEngine{{40, 512, kiyosi::finite_difference_scheme::crank_nicolson, 110.0}}
                    .price(phoenix, context));
}

}
