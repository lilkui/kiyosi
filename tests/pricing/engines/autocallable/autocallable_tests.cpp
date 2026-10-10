#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <catch2/generators/catch_generators.hpp>

#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <future>
#include <limits>
#include <memory>
#include <optional>
#include <random>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"
#include "pricing/engines/monte_carlo_random.hpp"

namespace {

using kiyosi::test::day;

TEST_CASE("Knocked-in finite-difference autocallables preserve coupons and downside", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), valuation = day(2025, 1, 2), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 70.0, valuation, all_days_calendar());
    const auto put_price = [&](double strike) {
        return *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::put, strike, start, end), context);
    };
    const auto discount = [&](Date date) { return std::exp(-0.04 * *year_fraction(valuation, date)); };
    const double downside = discount(end) - (put_price(100.0) - put_price(60.0)) / 100.0;
    const double coupons = 0.08 * (*year_fraction(start, valuation) +
                                   *year_fraction(valuation, middle) * discount(middle) +
                                   *year_fraction(middle, end) * discount(end));
    for (const auto state : {AutocallableBarrierState::none, AutocallableBarrierState::knocked_in}) {
        const auto snowball = *make_snowball_option(
            {.knock_out_coupon_rates = {0.1, 0.1, 0.1}, .maturity_coupon_rate = 0.8, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {200.0, 200.0, 200.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {valuation, middle, end}, .barrier_state = state, .effective_date = start, .expiry_date = end});
        const auto phoenix = *make_phoenix_option(
            {.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {200.0, 200.0, 200.0}, .coupon_barrier_levels = {0.0, 0.0, 0.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {valuation, middle, end}, .barrier_state = state, .effective_date = start, .expiry_date = end});
        const auto ternary = *make_ternary_snowball_option(
            {.knock_out_coupon_rates = {0.1, 0.1, 0.1}, .maturity_coupon_rate = 0.8, .minimum_coupon_rate = 0.02, .knock_in_level = 75.0, .knock_out_levels = {200.0, 200.0, 200.0}, .observation_dates = {valuation, middle, end}, .barrier_state = state, .effective_date = start, .expiry_date = end});
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler}) {
            const FiniteDifferenceSettings settings{.asset_step_count = 800, .time_step_count = 400, .scheme = scheme, .asset_upper_boundary = 800.0};
            const auto check = [&](const auto& note, double expected) {
                const FiniteDifferenceAutocallableEngine<std::remove_cvref_t<decltype(note)>> engine{settings};
                const auto price = engine.price(note, context);
                const auto result = engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma});
                REQUIRE(price);
                REQUIRE(result);
                CHECK(*price == Catch::Approx(expected).margin(0.0002));
                CHECK(result->price() == *price);
                CHECK(result->has(Greek::delta));
                CHECK(result->has(Greek::gamma));
            };
            CAPTURE(state, scheme);
            check(snowball, downside);
            check(phoenix, downside + coupons);
            check(ternary, 1.02 * discount(end));
        }
    }
}

TEST_CASE("Autocallable finite-difference spot Greeks reuse one solve", "[fd-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 70.0,
                                               day(2025, 1, 2), calendar);
    const auto note = *make_snowball_option(
        {.knock_out_coupon_rates = {0.0}, .maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {200.0}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {end}, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end});
    const FiniteDifferenceSnowballEngine engine{{.asset_step_count = 800, .time_step_count = 200, .asset_upper_boundary = 800.0}};
    const auto price = engine.price(note, context);
    REQUIRE(price);
    const int price_queries = queries->exchange(0);
    const auto result = engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma});
    REQUIRE(result);
    CHECK(queries->load() == price_queries);
    CHECK(result->price() == *price);
    const auto put = *make_european_option(OptionType::put, 100.0, start, end);
    const auto expected = AnalyticVanillaEngine{}.price_with_greeks(put, context, {Greek::delta, Greek::gamma});
    REQUIRE(expected);
    CHECK(test::greek_value(*result, Greek::delta) == Catch::Approx(-test::greek_value(*expected, Greek::delta) / 100.0).margin(1e-5));
    CHECK(test::greek_value(*result, Greek::gamma) == Catch::Approx(-test::greek_value(*expected, Greek::gamma) / 100.0).margin(1e-6));
    CHECK_FALSE(result->has(Greek::vega));
}

TEST_CASE("Knocked-in autocallables simulate only remaining observation dates", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), valuation = day(2025, 1, 2), end = day(2026, 1, 1);
    const auto queries = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 70.0, valuation, calendar);
    const auto note = *make_snowball_option(
        {.knock_out_coupon_rates = {0.0}, .maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {1e6}, .upper_strike = 100.0, .lower_strike = 0.0, .observation_dates = {end}, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end});
    const auto actual = MonteCarloSnowballEngine{{50'000, 42}}.price(note, context);
    REQUIRE(actual);
    // Calendar validation remains necessary; daily continuation queries do not.
    CHECK(queries->load() < 10);
    const auto put = *make_european_option(OptionType::put, 100.0, start, end);
    const auto expected = AnalyticVanillaEngine{}.price(put, context);
    REQUIRE(expected);
    CHECK(*actual == Catch::Approx(1.0 - *expected / 100.0).margin(0.002));
}

TEST_CASE("Seeded autocallable paths stay coupled when knock-out dates change")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.0, 0.0},
                                                            .maturity_coupon_rate = 0.2,
                                                            .knock_out_levels = {110.0, 110.0},
                                                            .observation_dates = {day(2025, 7, 1), end},
                                                            .effective_date = start,
                                                            .expiry_date = end});
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    for (const std::uint64_t seed : {std::uint64_t{1}, std::numeric_limits<std::uint64_t>::max()}) {
        CAPTURE(seed);
        const kiyosi::MonteCarloBinarySnowballEngine engine{{2000, seed}};
        double previous = 1.2;
        for (int bump = 0; bump <= 30; ++bump) {
            const double spot = 100.0 + 0.01 * bump;
            CAPTURE(spot);
            const auto context = *kiyosi::make_pricing_context(parameters, spot, start);
            const auto actual = engine.price(note, context);
            REQUIRE(actual);
            // On the same paths, earlier zero-coupon knock-out can only reduce this payout.
            CHECK(*actual <= previous);
            previous = *actual;
        }
    }
}

TEST_CASE("Autocallable engines validate observation dates against each market calendar")
{
    const auto effective = day(2025, 1, 3);
    const auto expiry = day(2025, 1, 6);
    const auto note = *kiyosi::make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1, .knock_out_levels = {120.0}, .observation_dates = {day(2025, 1, 4)}, .effective_date = effective, .expiry_date = expiry});
    for (const auto& calendar : {kiyosi::all_days_calendar(), kiyosi::weekdays_calendar()}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), 100.0, effective, calendar);
        for (const auto& result : {
                 kiyosi::MonteCarloBinarySnowballEngine{{32, 7}}.price(note, context),
                 kiyosi::FiniteDifferenceBinarySnowballEngine{{40, 40}}.price(note, context)}) {
            if (calendar.is_trading_day(day(2025, 1, 4))) {
                REQUIRE(result);
                CHECK(std::isfinite(*result));
            } else {
                REQUIRE_FALSE(result);
                CHECK(result.error().category == kiyosi::ErrorCategory::invalid_date);
                CHECK(result.error().message == "observation Date is not a trading day");
            }
        }
    }
}

TEST_CASE("Autocallable engines reject nominal weekend expiry and accept adjusted expiry")
{
    const auto nominal = day(2025, 1, 5);
    const auto calendar = kiyosi::weekdays_calendar();
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), 100.0, day(2025, 1, 3), calendar);
    const auto note = [&](kiyosi::Date expiry) {
        return *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.0}, .maturity_coupon_rate = 0.01, .knock_out_levels = {200.0}, .observation_dates = {day(2025, 1, 3)}, .effective_date = day(2025, 1, 3), .expiry_date = expiry});
    };
    const auto unadjusted = note(nominal);
    const auto adjusted_expiry = calendar.adjust(nominal, kiyosi::BusinessDayConvention::following);
    REQUIRE(adjusted_expiry);
    const auto adjusted = note(*adjusted_expiry);
    const kiyosi::MonteCarloBinarySnowballEngine mc{{32, 7}};
    const kiyosi::FiniteDifferenceBinarySnowballEngine fd{};
    for (const auto& result : {mc.price(unadjusted, context), fd.price(unadjusted, context)}) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == kiyosi::ErrorCategory::invalid_date);
    }
    CHECK(mc.price(adjusted, context).has_value());
    CHECK(fd.price(adjusted, context).has_value());
}

double reference_binary_snowball_price(const kiyosi::BinarySnowballOption& note,
                                       const kiyosi::PricingContext& context,
                                       kiyosi::TradingDayMonteCarloSettings settings)
{
    const auto path_payoff = [&](kiyosi::detail::Pcg32& generator) {
        if (note.barrier_state() == kiyosi::AutocallableBarrierState::knocked_out) return 0.0;

        const double rate = context.model_parameters().risk_free_rate();
        const double dividend = context.model_parameters().dividend_yield();
        const double sigma = context.model_parameters().volatility();
        const auto valuation = context.valuation_time();
        const auto& dates = note.observation_dates();
        std::vector<std::size_t> schedule;
        for (std::size_t index = 0; index < dates.size(); ++index)
            if (dates[index] >= valuation) schedule.push_back(index);

        double value = context.spot_price();
        std::size_t index = 0;
        if (!schedule.empty() && dates[schedule.front()] == valuation) {
            const auto event = schedule.front();
            const double coupon = note.knock_out_coupon_rates()[event] *
                                  *kiyosi::year_fraction(note.effective_date(), dates[event]);
            if (value >= note.knock_out_levels()[event]) return note.principal_ratio() + coupon;
            index = 1;
        }
        const double terminal = note.principal_ratio() +
                                note.maturity_coupon_rate() *
                                    *kiyosi::year_fraction(note.effective_date(), note.expiry_date());
        if (valuation == note.expiry_date()) return terminal;

        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (; index < schedule.size(); ++index) {
            const auto current = dates[schedule[index]];
            const double dt = *kiyosi::year_fraction(previous, kiyosi::start_of_day(current));
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                              sigma * std::sqrt(dt) * normal(generator));
            previous = kiyosi::start_of_day(current);
            const auto event = schedule[index];
            const double coupon = note.knock_out_coupon_rates()[event] *
                                  *kiyosi::year_fraction(note.effective_date(), dates[event]);
            if (value >= note.knock_out_levels()[event])
                return (note.principal_ratio() + coupon) *
                       std::exp(-rate * *kiyosi::year_fraction(valuation, previous));
        }
        return std::exp(-rate *
                        *kiyosi::year_fraction(valuation, kiyosi::start_of_day(note.expiry_date()))) *
               terminal;
    };

    std::mt19937_64 generator(settings.seed.value_or(0));
    double sum = 0.0;
    for (int path = 0; path < settings.path_count; ++path) {
        kiyosi::detail::Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
        sum += path_payoff(path_generator);
    }
    return sum / static_cast<double>(settings.path_count);
}

TEST_CASE("Phoenix expiry_date settlement applies state and final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloAutocallableEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result;
    };

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 80.0,
                                                       .knock_out_levels = {100.0},
                                                       .coupon_barrier_levels = {90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {expiry_date},
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    CHECK(price(phoenix, 100.0) == Catch::Approx(1.08));
    CHECK(price(phoenix, 80.0) == Catch::Approx(1.0));
    const auto phoenix_up = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                          .initial_spot = 100.0,
                                                          .knock_in_level = 80.0,
                                                          .knock_out_levels = {100.0, 100.0},
                                                          .coupon_barrier_levels = {90.0, 90.0},
                                                          .upper_strike = 100.0,
                                                          .lower_strike = 60.0,
                                                          .observation_dates = {day(2025, 7, 1), expiry_date},
                                                          .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                          .barrier_state = kiyosi::AutocallableBarrierState::knocked_out,
                                                          .principal_ratio = 1.0,
                                                          .effective_date = effective_date,
                                                          .expiry_date = expiry_date});
    CHECK(price(phoenix_up, 100.0) == 0.0);
}

TEST_CASE("Snowball expiry_date settlement applies state and final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloAutocallableEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result;
    };
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.10},
                                                         .maturity_coupon_rate = 0.05,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 80.0,
                                                         .knock_out_levels = {100.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {expiry_date},
                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                         .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                         .principal_ratio = 1.0,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    CHECK(price(snowball, 100.0) == Catch::Approx(1.10));
    CHECK(price(snowball, 80.0) == Catch::Approx(1.05));
    CHECK(price(snowball, 59.0) == Catch::Approx(0.60));
    const auto snowball_down = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.10},
                                                              .maturity_coupon_rate = 0.05,
                                                              .initial_spot = 100.0,
                                                              .knock_in_level = 80.0,
                                                              .knock_out_levels = {110.0},
                                                              .upper_strike = 100.0,
                                                              .lower_strike = 60.0,
                                                              .observation_dates = {expiry_date},
                                                              .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                              .barrier_state = kiyosi::AutocallableBarrierState::knocked_in,
                                                              .principal_ratio = 1.0,
                                                              .effective_date = effective_date,
                                                              .expiry_date = expiry_date});
    CHECK(price(snowball_down, 70.0) == Catch::Approx(0.70));
}

TEST_CASE("Autocallable Monte Carlo skips knock-in on a non-trading valuation day")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2025, 1, 6);
    const auto note = *kiyosi::make_snowball_option(
        {.knock_out_coupon_rates = {0.12}, .maturity_coupon_rate = 0.12, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry}, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .effective_date = effective, .expiry_date = expiry});
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto saturday = kiyosi::start_of_day(day(2025, 1, 4));
    const auto midnight = *kiyosi::make_pricing_context(parameters, 79.0, saturday);
    const auto later = *kiyosi::make_pricing_context(parameters, 79.0, saturday + std::chrono::microseconds{1});
    const kiyosi::MonteCarloSnowballEngine engine{{20'000, 1}};
    const auto first = engine.price(note, midnight);
    const auto second = engine.price(note, later);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(std::abs(*first - *second) < 1e-7);
}

TEST_CASE("Binary snowball expiry_date settlement applies final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& engine, const auto& note, double spot) {
        const auto result = engine.price(note, market(spot));
        REQUIRE(result);
        return *result;
    };
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10},
                                                              .maturity_coupon_rate = 0.05,
                                                              .knock_out_levels = {100.0},
                                                              .observation_dates = {expiry_date},
                                                              .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                              .principal_ratio = 1.0,
                                                              .effective_date = effective_date,
                                                              .expiry_date = expiry_date});
    const auto check = [&](const auto& engine) {
        CHECK(price(engine, binary, 100.0) == Catch::Approx(1.10));
        CHECK(price(engine, binary, 90.0) == Catch::Approx(1.05));
    };
    check(kiyosi::MonteCarloBinarySnowballEngine{{32, 7}});
    check(kiyosi::FiniteDifferenceBinarySnowballEngine{});
}

TEST_CASE("Ternary snowball expiry_date settlement applies final observations")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             spot, expiry_date);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloAutocallableEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result;
    };
    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.10},
                                                                .maturity_coupon_rate = 0.05,
                                                                .minimum_coupon_rate = 0.02,
                                                                .knock_in_level = 80.0,
                                                                .knock_out_levels = {100.0},
                                                                .observation_dates = {expiry_date},
                                                                .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                                .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                .principal_ratio = 1.0,
                                                                .effective_date = effective_date,
                                                                .expiry_date = expiry_date});
    CHECK(price(ternary, 100.0) == Catch::Approx(1.10));
    CHECK(price(ternary, 80.0) == Catch::Approx(1.05));
    CHECK(price(ternary, 79.0) == Catch::Approx(1.02));
}

TEST_CASE("Autocallable Monte Carlo processes valuation-date observation events once")
{
    const auto effective_date = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0, valuation);
    const auto monte_carlo_price = [&](const auto& instrument) {
        using Instrument = std::remove_cvref_t<decltype(instrument)>;
        const auto result = kiyosi::MonteCarloAutocallableEngine<Instrument>{{32, 7}}.price(instrument, context);
        REQUIRE(result);
        return *result;
    };
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {99.0, 10.0, 0.10},
                                                            .maturity_coupon_rate = 0.05,
                                                            .knock_out_levels = {90.0, 90.0, 99.0},
                                                            .observation_dates = {effective_date, valuation, expiry_date},
                                                            .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                            .principal_ratio = 1.0,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const kiyosi::MonteCarloBinarySnowballEngine engine{{32, 7}};
    const auto first = engine.price(note, context);
    const auto second = engine.price(note, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(*first == *second);
    CHECK(*first ==
          Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective_date, valuation).value()).epsilon(0.0).margin(1e-10));
    for (const auto elapsed : {std::chrono::hours{12}, std::chrono::hours{24}}) {
        const auto later_context = *kiyosi::make_pricing_context(
            context.model_parameters(), 100.0, kiyosi::start_of_day(valuation) + elapsed);
        const auto later = engine.price(note, later_context);
        REQUIRE(later);
        CHECK(*later == Catch::Approx(
                            1.0 + 0.10 * kiyosi::year_fraction(effective_date, expiry_date).value())
                            .epsilon(0.0)
                            .margin(1e-10));
    }
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {99.0, 10.0, 0.10},
                                                         .maturity_coupon_rate = 0.05,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 80.0,
                                                         .knock_out_levels = {90.0, 90.0, 99.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {effective_date, valuation, expiry_date},
                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                         .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                         .principal_ratio = 1.0,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    CHECK(monte_carlo_price(snowball) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective_date, valuation).value()));

    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {99.0, 10.0, 0.10},
                                                                .maturity_coupon_rate = 0.05,
                                                                .minimum_coupon_rate = 0.02,
                                                                .knock_in_level = 80.0,
                                                                .knock_out_levels = {90.0, 90.0, 99.0},
                                                                .observation_dates = {effective_date, valuation, expiry_date},
                                                                .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                                .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                .principal_ratio = 1.0,
                                                                .effective_date = effective_date,
                                                                .expiry_date = expiry_date});
    CHECK(monte_carlo_price(ternary) == Catch::Approx(1.0 + 10.0 * kiyosi::year_fraction(effective_date, valuation).value()));

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 80.0,
                                                       .knock_out_levels = {90.0, 90.0, 99.0},
                                                       .coupon_barrier_levels = {90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {effective_date, valuation, expiry_date},
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    CHECK(monte_carlo_price(phoenix) ==
          Catch::Approx(1.0 + 0.08 * kiyosi::year_fraction(effective_date, valuation).value()));
}

TEST_CASE("Autocallable Monte Carlo settles deterministic states before simulation")
{
    const auto effective_date = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), 100.0, valuation, calendar);
    const auto make_note = [&](kiyosi::AutocallableBarrierState barrier_state, double principal,
                               double coupon = 0.0) {
        return *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {coupon, 0.0},
                                                     .maturity_coupon_rate = 0.0,
                                                     .knock_out_levels = {100.0, 100.0},
                                                     .observation_dates = {valuation, expiry_date},
                                                     .barrier_state = barrier_state,
                                                     .principal_ratio = principal,
                                                     .effective_date = effective_date,
                                                     .expiry_date = expiry_date});
    };

    const double large_payoff = std::numeric_limits<double>::max() / 64.0;
    const auto immediate = kiyosi::MonteCarloBinarySnowballEngine{{128, 7}}.price(
        make_note(kiyosi::AutocallableBarrierState::none, large_payoff), context);
    REQUIRE(immediate);
    CHECK(*immediate == large_payoff);
    CHECK(calls->load() == 3);

    calls->store(0);
    const auto touched = kiyosi::MonteCarloBinarySnowballEngine{{128, 7}}.price(
        make_note(kiyosi::AutocallableBarrierState::knocked_out, 1.0), context);
    REQUIRE_FALSE(touched);
    CHECK(touched.error().category == kiyosi::ErrorCategory::invalid_option);
    CHECK(calls->load() == 2);

    calls->store(0);
    const auto expiry_context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2), 90.0, expiry_date, calendar);
    const auto expiry_note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.0},
                                                                   .maturity_coupon_rate = 0.0,
                                                                   .knock_out_levels = {100.0},
                                                                   .observation_dates = {expiry_date},
                                                                   .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                   .principal_ratio = large_payoff,
                                                                   .effective_date = effective_date,
                                                                   .expiry_date = expiry_date});
    const auto at_expiry = kiyosi::MonteCarloBinarySnowballEngine{{128, 7}}.price(
        expiry_note, expiry_context);
    REQUIRE(at_expiry);
    CHECK(*at_expiry == large_payoff);
    CHECK(calls->load() == 2);

    calls->store(0);
    const auto invalid_settings = kiyosi::MonteCarloBinarySnowballEngine{{0, 7}}.price(
        make_note(kiyosi::AutocallableBarrierState::none, 1.0), context);
    REQUIRE_FALSE(invalid_settings);
    CHECK(invalid_settings.error().category == kiyosi::ErrorCategory::invalid_parameter);
    CHECK(calls->load() == 2);

    const auto non_finite = kiyosi::MonteCarloBinarySnowballEngine{{128, 7}}.price(
        make_note(kiyosi::AutocallableBarrierState::none, std::numeric_limits<double>::max(),
                  std::numeric_limits<double>::max()),
        context);
    REQUIRE_FALSE(non_finite);
    CHECK(non_finite.error().category == kiyosi::ErrorCategory::invalid_result);
}

TEST_CASE("Autocallable Monte Carlo prepares stable calendar inputs once")
{
    const auto valuation = day(2025, 1, 1);
    const auto first_observation = day(2025, 1, 4);
    const auto expiry_date = day(2025, 1, 6);
    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date value) {
            ++*calls;
            return value == valuation || value == day(2025, 1, 2) ||
                   value == first_observation || value == expiry_date;
        },
        252);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2), 100.0, valuation, calendar);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10, 0.20},
                                                            .maturity_coupon_rate = 0.02,
                                                            .knock_out_levels = {100.0, 100.0},
                                                            .observation_dates = {first_observation, expiry_date},
                                                            .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                            .principal_ratio = 1.0,
                                                            .effective_date = valuation,
                                                            .expiry_date = expiry_date});
    const std::array seeds{std::uint64_t{0}, std::numeric_limits<std::uint64_t>::max()};
    for (const std::uint64_t seed : seeds) {
        CAPTURE(seed);
        const kiyosi::TradingDayMonteCarloSettings settings{32, seed};
        const double expected = reference_binary_snowball_price(note, context, settings);
        calls->store(0);

        const auto result = kiyosi::MonteCarloBinarySnowballEngine{settings}.price(note, context);

        REQUIRE(result);
        CHECK(*result == Catch::Approx(expected).epsilon(0.0).margin(1e-12));
        CHECK(calls->load() == 3);
    }

    const auto unseeded =
        kiyosi::MonteCarloBinarySnowballEngine{{32, std::nullopt}}.price(note, context);
    REQUIRE(unseeded);
    CHECK(std::isfinite(*unseeded));
}

TEST_CASE("Autocallable CUDA selection validates and preserves deterministic settlements")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 6);
    const auto make_note = [&](kiyosi::AutocallableBarrierState barrier_state) {
        return *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                     .maturity_coupon_rate = 0.05,
                                                     .knock_out_levels = {110.0},
                                                     .observation_dates = {expiry_date},
                                                     .barrier_state = barrier_state,
                                                     .principal_ratio = 1.0,
                                                     .effective_date = effective_date,
                                                     .expiry_date = expiry_date});
    };
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.01, 0.2), 100.0, effective_date,
        kiyosi::all_days_calendar());
    const kiyosi::MonteCarloBinarySnowballEngine cuda{
        {64, 7, kiyosi::MonteCarloBackend::cuda}};
    CHECK(cuda.settings().backend == kiyosi::MonteCarloBackend::cuda);

    const auto settled_note = *kiyosi::make_binary_snowball_option(
        {.knock_out_coupon_rates = {0.08, 0.08}, .maturity_coupon_rate = 0.05, .knock_out_levels = {110.0, 110.0}, .observation_dates = {day(2025, 1, 2), expiry_date}, .barrier_state = kiyosi::AutocallableBarrierState::knocked_out, .effective_date = effective_date, .expiry_date = expiry_date});
    const auto settled_context = *kiyosi::make_pricing_context(
        context.model_parameters(), 100.0, day(2025, 1, 3), kiyosi::all_days_calendar());
    const auto settled = cuda.price(settled_note, settled_context);
    REQUIRE(settled);
    CHECK(*settled == 0.0);

    const auto invalid = kiyosi::MonteCarloBinarySnowballEngine{
        {0, 7, kiyosi::MonteCarloBackend::cuda}}
                             .price(make_note(kiyosi::AutocallableBarrierState::none), context);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto invalid_backend = kiyosi::MonteCarloBinarySnowballEngine{
        {64, 7, static_cast<kiyosi::MonteCarloBackend>(255)}}
                                     .price(make_note(kiyosi::AutocallableBarrierState::none), context);
    REQUIRE_FALSE(invalid_backend);
    CHECK(invalid_backend.error().category == kiyosi::ErrorCategory::invalid_parameter);

#if !KIYOSI_HAS_CUDA
    const auto unavailable =
        cuda.price(make_note(kiyosi::AutocallableBarrierState::none), context);
    REQUIRE_FALSE(unavailable);
    CHECK(unavailable.error().category == kiyosi::ErrorCategory::backend_unavailable);
#endif
}

#if KIYOSI_HAS_CUDA
TEST_CASE("Autocallable CUDA Monte Carlo prices every public autocallable engine")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 4, 1);
    const std::vector<kiyosi::Date> observations{
        day(2025, 2, 1), day(2025, 3, 1), expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.03, 0.01, 0.2), 100.0, effective_date,
        kiyosi::all_days_calendar());
    const kiyosi::TradingDayMonteCarloSettings cpu_settings{
        32'768, 73, kiyosi::MonteCarloBackend::cpu};
    const kiyosi::TradingDayMonteCarloSettings cuda_settings{
        32'768, 73, kiyosi::MonteCarloBackend::cuda};
    const auto check = [&](const auto& note, const auto& cpu_engine, const auto& cuda_engine) {
        const auto cpu = cpu_engine.price(note, context);
        auto first_future = std::async(
            std::launch::async, [&] { return cuda_engine.price(note, context); });
        auto second_future = std::async(
            std::launch::async, [&] { return cuda_engine.price(note, context); });
        const auto first = first_future.get();
        const auto second = second_future.get();
        REQUIRE(cpu);
        REQUIRE(first);
        REQUIRE(second);
        const double cuda_price = *first;
        CHECK(cuda_price == *second);
        CHECK(cuda_price == Catch::Approx(*cpu)
                                .margin(0.04));
    };

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.002,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 80.0,
                                                       .knock_out_levels = {110.0, 108.0, 105.0},
                                                       .coupon_barrier_levels = {95.0, 95.0, 95.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = observations,
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    check(phoenix, kiyosi::MonteCarloPhoenixEngine{cpu_settings},
          kiyosi::MonteCarloPhoenixEngine{cuda_settings});

    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08},
                                                         .maturity_coupon_rate = 0.06,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 80.0,
                                                         .knock_out_levels = {110.0, 108.0, 105.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = observations,
                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                         .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                         .principal_ratio = 1.0,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    check(snowball, kiyosi::MonteCarloSnowballEngine{cpu_settings},
          kiyosi::MonteCarloSnowballEngine{cuda_settings});

    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08},
                                                              .maturity_coupon_rate = 0.06,
                                                              .knock_out_levels = {110.0, 108.0, 105.0},
                                                              .observation_dates = observations,
                                                              .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                              .principal_ratio = 1.0,
                                                              .effective_date = effective_date,
                                                              .expiry_date = expiry_date});
    check(binary, kiyosi::MonteCarloBinarySnowballEngine{cpu_settings},
          kiyosi::MonteCarloBinarySnowballEngine{cuda_settings});

    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08},
                                                                .maturity_coupon_rate = 0.06,
                                                                .minimum_coupon_rate = 0.01,
                                                                .knock_in_level = 80.0,
                                                                .knock_out_levels = {110.0, 108.0, 105.0},
                                                                .observation_dates = observations,
                                                                .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry,
                                                                .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                .principal_ratio = 1.0,
                                                                .effective_date = effective_date,
                                                                .expiry_date = expiry_date});
    check(ternary, kiyosi::MonteCarloTernarySnowballEngine{cpu_settings},
          kiyosi::MonteCarloTernarySnowballEngine{cuda_settings});

    const auto different_seed = kiyosi::MonteCarloPhoenixEngine{
        {32'768, 74, kiyosi::MonteCarloBackend::cuda}}
                                    .price(phoenix, context);
    const auto original_seed =
        kiyosi::MonteCarloPhoenixEngine{cuda_settings}.price(phoenix, context);
    REQUIRE(different_seed);
    REQUIRE(original_seed);
    CHECK(*different_seed !=
          *original_seed);
}

TEST_CASE("Autocallable CUDA Monte Carlo preserves coupons and historical touch state")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2025, 1, 4);
    const auto context = [&](double spot, kiyosi::Date valuation) {
        return *kiyosi::make_pricing_context( // NOLINT(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar; MSVC std::function copies are misdiagnosed.
            *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-8), spot, valuation,
            kiyosi::all_days_calendar());
    };
    const kiyosi::TradingDayMonteCarloSettings settings{
        4'096, 73, kiyosi::MonteCarloBackend::cuda};

    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.01,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 50.0,
                                                       .knock_out_levels = {200.0, 200.0, 200.0},
                                                       .coupon_barrier_levels = {90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {day(2025, 1, 2), day(2025, 1, 3), expiry_date},
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    const auto phoenix_result =
        kiyosi::MonteCarloPhoenixEngine{settings}.price(phoenix, context(100.0, effective_date));
    REQUIRE(phoenix_result);
    CHECK(*phoenix_result ==
          Catch::Approx(1.0 + 0.01 * 3.0 / 365.0).margin(1e-10).epsilon(0.0));

    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.08},
                                                         .maturity_coupon_rate = 0.06,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 80.0,
                                                         .knock_out_levels = {200.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {expiry_date},
                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                         .barrier_state = kiyosi::AutocallableBarrierState::knocked_in,
                                                         .principal_ratio = 1.0,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    const auto snowball_result =
        kiyosi::MonteCarloSnowballEngine{settings}.price(snowball, context(70.0, day(2025, 1, 2)));
    REQUIRE(snowball_result);
    CHECK(*snowball_result ==
          Catch::Approx(0.7).epsilon(0.0).margin(1e-8));

    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                                .maturity_coupon_rate = 0.06,
                                                                .minimum_coupon_rate = 0.01,
                                                                .knock_in_level = 80.0,
                                                                .knock_out_levels = {200.0},
                                                                .observation_dates = {expiry_date},
                                                                .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                                .barrier_state = kiyosi::AutocallableBarrierState::knocked_in,
                                                                .principal_ratio = 1.0,
                                                                .effective_date = effective_date,
                                                                .expiry_date = expiry_date});
    const auto ternary_result =
        kiyosi::MonteCarloTernarySnowballEngine{settings}.price(ternary, context(100.0, day(2025, 1, 2)));
    REQUIRE(ternary_result);
    CHECK(*ternary_result ==
          Catch::Approx(1.0 + 0.01 * 3.0 / 365.0).epsilon(0.0).margin(1e-12));

    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                              .maturity_coupon_rate = 0.06,
                                                              .knock_out_levels = {200.0},
                                                              .observation_dates = {expiry_date},
                                                              .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                              .principal_ratio = 1.0,
                                                              .effective_date = effective_date,
                                                              .expiry_date = expiry_date});
    const auto binary_result =
        kiyosi::MonteCarloBinarySnowballEngine{settings}.price(binary, context(100.0, effective_date));
    REQUIRE(binary_result);
    CHECK(*binary_result ==
          Catch::Approx(1.0 + 0.06 * 3.0 / 365.0).epsilon(0.0).margin(1e-12));
}
#endif

TEST_CASE("Autocallable finite difference preserves future observation indices")
{
    const auto effective_date = day(2025, 1, 1);
    const auto valuation_date = day(2025, 1, 2);
    const auto future_observation = day(2025, 1, 4);
    const auto expiry_date = day(2025, 1, 6);
    const auto note = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {100.0, 0.2, 0.3},
                                                            .maturity_coupon_rate = 0.01,
                                                            .knock_out_levels = {90.0, 90.0, 90.0},
                                                            .observation_dates = {valuation_date, future_observation, expiry_date},
                                                            .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                            .principal_ratio = 1.0,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), 100.0,
        kiyosi::start_of_day(valuation_date) + std::chrono::hours{12},
        kiyosi::all_days_calendar());

    const auto result = kiyosi::FiniteDifferenceBinarySnowballEngine{{100, 3}}.price(note, context);

    REQUIRE(result);
    const double expected = 1.0 + 0.2 * *kiyosi::year_fraction(effective_date, future_observation);
    CHECK(*result == Catch::Approx(expected).epsilon(0.0).margin(1e-12));
}

TEST_CASE("Binary snowball finite difference prices active and knocked-out states")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{
        day(2025, 2, 3), day(2025, 6, 17), day(2025, 11, 5), expiry_date};
    const auto note = [&](kiyosi::AutocallableBarrierState barrier_state) {
        return *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.02, 0.04, 0.06, 0.08},
                                                     .maturity_coupon_rate = 0.05,
                                                     .knock_out_levels = {112.0, 108.0, 104.0, 100.0},
                                                     .observation_dates = observation_dates,
                                                     .barrier_state = barrier_state,
                                                     .principal_ratio = 1.0,
                                                     .effective_date = effective_date,
                                                     .expiry_date = expiry_date});
    };
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 97.0, effective_date);

    for (const auto scheme : {kiyosi::FiniteDifferenceScheme::explicit_euler,
                              kiyosi::FiniteDifferenceScheme::implicit_euler,
                              kiyosi::FiniteDifferenceScheme::crank_nicolson}) {
        CAPTURE(scheme);
        const kiyosi::FiniteDifferenceBinarySnowballEngine engine{{80, 512, scheme, 500.0}};
        const auto untouched = engine.price(note(kiyosi::AutocallableBarrierState::none), context);
        const auto up_touched = engine.price(note(kiyosi::AutocallableBarrierState::knocked_out), context);

        REQUIRE(untouched);
        REQUIRE_FALSE(up_touched);
        CHECK(*untouched > 0.0);
        CHECK(up_touched.error().category == kiyosi::ErrorCategory::invalid_option);
    }
}

TEST_CASE("Autocallable engines enumerate dates only for daily monitoring")
{
    const auto valuation = day(2025, 1, 1);
    const auto first_observation = day(2025, 1, 4);
    const auto expiry_date = day(2025, 1, 6);
    const auto parameters = *kiyosi::make_bsm_parameters(0.03, 0.0, 0.2);
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10, 0.20},
                                                              .maturity_coupon_rate = 0.02,
                                                              .knock_out_levels = {110.0, 110.0},
                                                              .observation_dates = {first_observation, expiry_date},
                                                              .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                              .principal_ratio = 1.0,
                                                              .effective_date = valuation,
                                                              .expiry_date = expiry_date});
    const auto make_snowball = [&](kiyosi::KnockInObservationMode knock_in_observation_mode) {
        return *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.10, 0.20},
                                              .maturity_coupon_rate = 0.02,
                                              .initial_spot = 100.0,
                                              .knock_in_level = 75.0,
                                              .knock_out_levels = {110.0, 110.0},
                                              .upper_strike = 100.0,
                                              .lower_strike = 60.0,
                                              .observation_dates = {first_observation, expiry_date},
                                              .knock_in_observation_mode = knock_in_observation_mode,
                                              .barrier_state = kiyosi::AutocallableBarrierState::none,
                                              .principal_ratio = 1.0,
                                              .effective_date = valuation,
                                              .expiry_date = expiry_date});
    };
    const bool monte_carlo = GENERATE(false, true);
    CAPTURE(monte_carlo);
    const auto price = [&](const auto& note, const auto& market) {
        using Note = std::remove_cvref_t<decltype(note)>;
        if (monte_carlo)
            return kiyosi::MonteCarloAutocallableEngine<Note>{{32, 7}}.price(note, market);
        return kiyosi::FiniteDifferenceAutocallableEngine<Note>{{40, 40}}.price(note, market);
    };
    const auto context = [&](kiyosi::TradingCalendar calendar) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        return *kiyosi::make_pricing_context(parameters, 100.0, valuation, std::move(calendar));
    };

    const auto calls = std::make_shared<std::atomic<int>>(0);
    const auto sparse_calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto sparse_context = context(sparse_calendar);

    const auto binary_result = price(binary, sparse_context);
    REQUIRE(binary_result);
    CHECK(calls->load() == 3);

    calls->store(0);
    const auto expiry_only = make_snowball(kiyosi::KnockInObservationMode::at_expiry);
    const auto expiry_result = price(expiry_only, sparse_context);
    REQUIRE(expiry_result);
    CHECK(calls->load() == 3);

    calls->store(0);
    const auto daily_calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date) {
            ++*calls;
            return true;
        },
        365);
    const auto daily_context = context(daily_calendar);
    const auto daily_result = price(make_snowball(kiyosi::KnockInObservationMode::every_trading_day), daily_context);
    REQUIRE(daily_result);
    CHECK(calls->load() == 9);

    calls->store(0);
    const auto invalid_calendar = *kiyosi::make_trading_calendar(
        [=](kiyosi::Date value) {
            ++*calls;
            return value != expiry_date;
        },
        365);
    const auto invalid_context = context(invalid_calendar);
    const auto invalid = price(binary, invalid_context);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_date);
    CHECK(calls->load() == (monte_carlo ? 2 : 1));
}

TEST_CASE("Finite-difference binary snowball engine rejects unstable explicit grids")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                      day(2025, 10, 1), expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = coupons,
                                                              .maturity_coupon_rate = 0.08,
                                                              .knock_out_levels = knock_outs,
                                                              .observation_dates = observation_dates,
                                                              .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                              .principal_ratio = 1.0,
                                                              .effective_date = effective_date,
                                                              .expiry_date = expiry_date});
    CHECK_FALSE(kiyosi::FiniteDifferenceBinarySnowballEngine{{40, 1, kiyosi::FiniteDifferenceScheme::explicit_euler}}
                    .price(binary, context));
}

TEST_CASE("Finite-difference phoenix engine rejects domains below the barrier")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{day(2025, 4, 1), day(2025, 7, 1),
                                                      day(2025, 10, 1), expiry_date};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.02,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 75.0,
                                                       .knock_out_levels = knock_outs,
                                                       .coupon_barrier_levels = {90.0, 90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = observation_dates,
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    CHECK_FALSE(kiyosi::FiniteDifferencePhoenixEngine{{40, 512, kiyosi::FiniteDifferenceScheme::crank_nicolson, 110.0}}
                    .price(phoenix, context));
}

} // namespace
