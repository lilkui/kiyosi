#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <future>
#include <limits>
#include <numbers>
#include <utility>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"
#include "support/numerical.hpp"

namespace {
using namespace kiyosi;
constexpr auto start = test::day(2025, 1, 1);
constexpr auto expiry = test::day(2026, 1, 1);
constexpr auto first = start + std::chrono::days{180};

PricingContext market(double spot = 100.0, Timestamp time = start_of_day(start), double volatility = 0.2,
                      double rate = 0.03, double dividend = 0.01)
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): the returned context owns its calendar.
    return *make_pricing_context(*make_bsm_parameters(rate, dividend, volatility), spot, time, all_days_calendar());
}

// Integrate the absorbing Brownian transition density in log spot, independently of the PDE.
std::array<double, 3> continuous_reference(double spot = 100.0, int count = 1024, double rate = 0.03, double dividend = 0.01)
{
    const double width = std::log(120.0 / 80.0), x = std::log(spot / 80.0);
    const double drift = rate - dividend - 0.5 * 0.2 * 0.2;
    const double alpha = drift / (0.2 * 0.2);
    double survival = 0.0, integral = 0.0;
    for (int n = 1; n <= count; ++n) {
        const double frequency = n * std::numbers::pi / width;
        const double coefficient = 2.0 / width * std::exp(-alpha * x) * std::sin(frequency * x) * frequency *
                                   (1.0 - (n % 2 == 0 ? 1.0 : -1.0) * std::exp(alpha * width)) / (alpha * alpha + frequency * frequency);
        const double decay = 0.5 * 0.2 * 0.2 * frequency * frequency + drift * drift / (2.0 * 0.2 * 0.2);
        survival += coefficient * std::exp(-decay);
        integral += coefficient * -std::expm1(-(rate + decay)) / (rate + decay);
    }
    const double untouched = std::exp(-rate) * survival;
    return {std::exp(-rate) - untouched, 1.0 - untouched - rate * integral, untouched};
}

struct CashReference {
    double price;
    double delta;
    double gamma;
    double speed;
};

// Differentiate the killed log-Brownian sine solution; no PDE grid or engine prices.
std::array<CashReference, 3> cash_reference(double spot, double volatility = 0.2, double rate = 0.03,
                                            double time = 1.0, int count = 64)
{
    const double width = std::log(120.0 / 80.0), x = std::log(spot / 80.0);
    const double alpha = (rate - 0.01 - 0.5 * volatility * volatility) / (volatility * volatility);
    const double beta = std::sqrt(alpha * alpha + 2.0 * rate / (volatility * volatility));
    const double plus = -alpha + beta, minus = -alpha - beta;
    const double weight = -std::expm1(minus * width) / (std::exp(plus * width) - std::exp(minus * width));
    std::array<double, 4> survival{}, transient{};
    for (int n = 1; n <= count; ++n) {
        const double k = n * std::numbers::pi / width;
        const double coefficient = 2.0 * k / width * (1.0 - (n % 2 == 0 ? 1.0 : -1.0) * std::exp(alpha * width));
        const double decay = 0.5 * volatility * volatility * (k * k + alpha * alpha) + rate;
        const double factor = coefficient * std::exp(-alpha * x - decay * time);
        const double sine = std::sin(k * x), cosine = std::cos(k * x);
        const std::array derivatives{sine, k * cosine - alpha * sine,
                                     (alpha * alpha - k * k) * sine - 2.0 * alpha * k * cosine,
                                     (3.0 * alpha * k * k - alpha * alpha * alpha) * sine +
                                         (3.0 * alpha * alpha * k - k * k * k) * cosine};
        for (std::size_t order = 0; order < derivatives.size(); ++order) {
            survival[order] += factor * derivatives[order] / (k * k + alpha * alpha);
            transient[order] += factor * derivatives[order] / (k * k + beta * beta);
        }
    }
    std::array<std::array<double, 4>, 3> values{};
    for (std::size_t order = 0; order < survival.size(); ++order) {
        values[0][order] = (order == 0 ? std::exp(-rate * time) : 0.0) - survival[order];
        // Subtract the transient from the stationary exit transform: its third derivatives converge.
        values[1][order] = weight * std::pow(plus, static_cast<int>(order)) * std::exp(plus * x) +
                           (1.0 - weight) * std::pow(minus, static_cast<int>(order)) * std::exp(minus * x) - transient[order];
        values[2][order] = survival[order];
    }
    const auto physical_derivatives = [&](const auto& value) {
        return CashReference{value[0], value[1] / spot, (value[2] - value[1]) / spot / spot,
                             (value[3] - 3.0 * value[2] + 2.0 * value[1]) / spot / spot / spot};
    };
    return {physical_derivatives(values[0]), physical_derivatives(values[1]), physical_derivatives(values[2])};
}

std::array<PricingResult, 3> reference_greeks(double spot)
{
    constexpr double shift = 0.0001;
    const auto base = cash_reference(spot);
    const auto vol_up = cash_reference(spot, 0.2 + shift), vol_down = cash_reference(spot, 0.2 - shift);
    const auto rate_up = cash_reference(spot, 0.2, 0.03 + shift), rate_down = cash_reference(spot, 0.2, 0.03 - shift);
    const auto after = cash_reference(spot, 0.2, 0.03, 1.0 - 1.0 / 365.0);
    const auto result = [&](std::size_t index) {
        const auto value = make_pricing_result(base[index].price,
                                               {{Greek::delta, base[index].delta}, {Greek::gamma, base[index].gamma}, {Greek::speed, base[index].speed}, {Greek::theta, after[index].price - base[index].price}, {Greek::charm, after[index].delta - base[index].delta}, {Greek::color, after[index].gamma - base[index].gamma}, {Greek::vega, (vol_up[index].price - vol_down[index].price) / (200.0 * shift)}, {Greek::vanna, (vol_up[index].delta - vol_down[index].delta) / (200.0 * shift)}, {Greek::zomma, (vol_up[index].gamma - vol_down[index].gamma) / (200.0 * shift)}, {Greek::rho, (rate_up[index].price - rate_down[index].price) / (200.0 * shift)}});
        REQUIRE(value);
        return *value;
    };
    return {result(0), result(1), result(2)};
}

double normal(double x) { return 0.5 * std::erfc(-x / std::numbers::sqrt2); }
double survival_at_fixing(double spot, double time)
{
    const double deviation = 0.2 * std::sqrt(time);
    return normal(std::log(120.125 / spot) / deviation) - normal(std::log(80.125 / spot) / deviation);
}

// One/two explicit fixings: integrate the second survival probability over the first fixing.
std::array<double, 3> scheduled_reference(const std::vector<Date>& dates, int count = 1024)
{
    const double time = detail::actual_365_fixed_year_fraction(start, dates.front());
    const double survived_first = survival_at_fixing(100.25, time);
    double survival = survived_first;
    if (dates.size() == 2) {
        const double deviation = 0.2 * std::sqrt(time);
        const double left = std::log(80.125 / 100.25) / deviation;
        const double right = std::log(120.125 / 100.25) / deviation;
        const double step = (right - left) / count;
        const auto integrand = [&](double z) {
            return survival_at_fixing(100.25 * std::exp(deviation * z), 1.0 - time) *
                   std::exp(-0.5 * z * z) / std::sqrt(2.0 * std::numbers::pi);
        };
        double total = integrand(left) + integrand(right);
        for (int index = 1; index < count; ++index)
            total += (index % 2 == 0 ? 2.0 : 4.0) * integrand(left + step * index);
        survival = step * total / 3.0;
    }
    return {std::exp(-0.03) * (1.0 - survival),
            std::exp(-0.03 * time) * (1.0 - survived_first) +
                std::exp(-0.03 * (dates.size() == 2 ? 1.0 : time)) * (survived_first - survival),
            std::exp(-0.03) * survival};
}

template <typename T>
void error_is(const Result<T>& result, ErrorCategory category)
{
    REQUIRE_FALSE(result);
    CHECK(result.error().category == category);
}
} // namespace

TEST_CASE("Double touch factories validate cash terms and own history", "[double-touch]")
{
    const auto option = *make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0);
    CHECK(option.is_one_touch());
    CHECK(option.settlement_timing() == default_one_touch_settlement_timing);
    CHECK(option.observation_mode() == default_touch_observation_mode);
    CHECK_FALSE(option.touch_state());
    CHECK(option.observation_dates().empty());
    const auto untouched = *option.replace(BarrierTouchState::untouched);
    CHECK(untouched.touch_state() == BarrierTouchState::untouched);
    CHECK_FALSE(option.touch_state());
    CHECK(*untouched.replace(std::nullopt) == option);
    for (const double invalid : {0.0, -1.0, std::numeric_limits<double>::infinity(), std::numeric_limits<double>::quiet_NaN()}) {
        error_is(make_cash_double_touch(start, expiry, 80.0, 120.0, invalid), ErrorCategory::invalid_parameter);
        error_is(make_cash_double_no_touch(start, expiry, invalid, 120.0, 1.0), ErrorCategory::invalid_parameter);
        error_is(make_cash_double_touch(start, expiry, 80.0, invalid, 1.0), ErrorCategory::invalid_parameter);
    }
    error_is(make_cash_double_no_touch(start, expiry, 120.0, 80.0, 1.0), ErrorCategory::invalid_parameter);
    error_is(make_cash_double_no_touch(start, expiry, 80.0, 80.0, 1.0), ErrorCategory::invalid_parameter);
    error_is(make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0, static_cast<SettlementTiming>(99)), ErrorCategory::invalid_option);
    error_is(make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0, SettlementTiming::at_expiry, static_cast<ObservationMode>(99)), ErrorCategory::invalid_schedule);
    error_is(option.replace(static_cast<BarrierTouchState>(99)), ErrorCategory::invalid_option);
    CHECK(*option.replace(std::nullopt) == option);
    error_is(make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0, ObservationMode::scheduled), ErrorCategory::invalid_schedule);
    error_is(make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0, SettlementTiming::at_expiry, ObservationMode::continuous, {expiry}), ErrorCategory::invalid_schedule);
    error_is(make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0, ObservationMode::scheduled, {expiry, first}), ErrorCategory::invalid_schedule);
    error_is(make_cash_double_no_touch(start, start - std::chrono::days{1}, 80.0, 120.0, 1.0), ErrorCategory::invalid_time_range);
    std::vector<Date> dates{first, expiry};
    const auto scheduled = *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0, ObservationMode::scheduled, dates);
    dates.clear();
    CHECK(scheduled.observation_dates() == std::vector<Date>{first, expiry});
    CHECK_FALSE(scheduled.is_one_touch());
    CHECK(scheduled.settlement_timing() == SettlementTiming::at_expiry);
}

TEST_CASE("Double touch continuous prices converge to independent cash references", "[double-touch]")
{
    const auto reference = continuous_reference();
    const auto check_reference = continuous_reference(100.0, 2048);
    const std::array options{*make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0),
                             *make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0, SettlementTiming::at_hit),
                             *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0)};
    for (std::size_t index = 0; index < options.size(); ++index) {
        CHECK_THAT(reference[index], Catch::Matchers::WithinAbs(check_reference[index], 1e-9));
        const auto coarse = *make_finite_difference_double_touch_engine({80, 100});
        const auto fine = *make_finite_difference_double_touch_engine({800, 1600});
        const auto actual = fine.price(options[index], market());
        const auto rough = coarse.price(options[index], market());
        REQUIRE(actual);
        REQUIRE(rough);
        CHECK(std::abs(*actual - reference[index]) < std::abs(*rough - reference[index]));
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(reference[index], 1e-5));
    }
    const auto fine = *make_finite_difference_double_touch_engine({800, 1600});
    const std::array scaled_options{*make_cash_double_touch(start, expiry, 80.0, 120.0, 7.5),
                                    *make_cash_double_touch(start, expiry, 80.0, 120.0, 7.5, SettlementTiming::at_hit),
                                    *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 7.5)};
    for (std::size_t index = 0; index < scaled_options.size(); ++index) {
        const auto actual = fine.price(scaled_options[index], market());
        REQUIRE(actual);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(7.5 * reference[index], 7.5e-5));
    }
    for (const auto& rates : {std::array{0.04, 0.0}, std::array{-0.01, 0.02}}) {
        const auto expected = continuous_reference(100.0, 1024, rates[0], rates[1]);
        for (std::size_t index = 0; index < options.size(); ++index) {
            const auto actual = fine.price(options[index], market(100.0, start_of_day(start), 0.2, rates[0], rates[1]));
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected[index], 1e-5));
        }
    }
    CHECK_THAT(*fine.price(options[0], market()) + *fine.price(options[2], market()), Catch::Matchers::WithinAbs(std::exp(-0.03), 1e-8));
}

TEST_CASE("Double touch scheduled prices match independent one and two fixing integrals", "[double-touch]")
{
    const auto engine = *make_finite_difference_double_touch_engine({1600, 1600, FiniteDifferenceScheme::crank_nicolson, 400.0});
    for (const auto& dates : {std::vector<Date>{expiry}, std::vector<Date>{first, expiry}, std::vector<Date>{first}}) {
        const auto reference = scheduled_reference(dates);
        const auto check_reference = scheduled_reference(dates, 2048);
        const std::array options{
            *make_cash_double_touch(start, expiry, 80.125, 120.125, 1.0, SettlementTiming::at_expiry, ObservationMode::scheduled, dates),
            *make_cash_double_touch(start, expiry, 80.125, 120.125, 1.0, SettlementTiming::at_hit, ObservationMode::scheduled, dates),
            *make_cash_double_no_touch(start, expiry, 80.125, 120.125, 1.0, ObservationMode::scheduled, dates)};
        for (std::size_t index = 0; index < options.size(); ++index) {
            CHECK_THAT(reference[index], Catch::Matchers::WithinAbs(check_reference[index], 1e-10));
            const auto actual = engine.price(options[index], market(100.25));
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(reference[index], 3e-5));
        }
    }
    const auto option = *make_cash_double_no_touch(start, expiry, 80.125, 120.125, 1.0, ObservationMode::scheduled, {expiry});
    const auto actual = engine.price(option, market(125.0));
    REQUIRE(actual);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(std::exp(-0.03) * survival_at_fixing(125.0, 1.0), 3e-5));
}

TEST_CASE("Double touch current and past payments preserve history and timing", "[double-touch]")
{
    const FiniteDifferenceDoubleTouchEngine engine;
    const auto tomorrow = start_of_day(start) + std::chrono::days{1};
    for (const auto timing : {SettlementTiming::at_hit, SettlementTiming::at_expiry}) {
        const auto option = *make_cash_double_touch(start, expiry, 80.0, 120.0, 3.0, timing);
        for (const double spot : {79.0, 80.0, 120.0, 121.0}) {
            const auto now = engine.price(option, market(spot));
            REQUIRE(now);
            CHECK_THAT(*now, Catch::Matchers::WithinAbs(timing == SettlementTiming::at_hit ? 3.0 : 3.0 * std::exp(-0.03), 1e-12));
            error_is(engine.price(option, market(spot, tomorrow)), ErrorCategory::invalid_parameter);
        }
        const auto known = *option.replace(BarrierTouchState::touched);
        error_is(engine.price(known, market()), ErrorCategory::invalid_option);
        const auto actual = engine.price(known, market(100.0, tomorrow));
        REQUIRE(actual);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(timing == SettlementTiming::at_hit ? 0.0 : 3.0 * std::exp(-0.03 * 364.0 / 365.0), 1e-12));
        error_is(implied_volatility(engine, known, market(100.0, tomorrow), *actual), ErrorCategory::unsupported_operation);
    }
    const auto no_touch = *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 3.0);
    for (const double spot : {80.0, 120.0})
        CHECK(*engine.price(no_touch, market(spot)) == 0.0);
    for (const auto& option : {no_touch, *make_cash_double_touch(start, expiry, 80.0, 120.0, 3.0),
                               *make_cash_double_touch(start, expiry, 80.0, 120.0, 3.0, SettlementTiming::at_hit)}) {
        const auto known = *option.replace(BarrierTouchState::untouched);
        CHECK(*engine.price(known, market(100.0, start_of_day(expiry))) == (known.is_one_touch() ? 0.0 : 3.0));
        for (const double spot : {80.0, 120.0})
            CHECK(*engine.price(known, market(spot, start_of_day(expiry))) == (known.is_one_touch() ? 3.0 : 0.0));
        error_is(engine.price(known, market(100.0, start_of_day(expiry) + std::chrono::seconds{1})), ErrorCategory::invalid_time_range);
    }
}

TEST_CASE("Double touch early observation windows retain exact UTC event semantics", "[double-touch]")
{
    const FiniteDifferenceDoubleTouchEngine engine;
    const auto option = *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 2.0, ObservationMode::scheduled, {first});
    const auto fixed = *option.replace(BarrierTouchState::untouched);
    const auto event = start_of_day(first);
    CHECK(*engine.price(option, market(120.0, event)) == 0.0);
    error_is(engine.price(option, market(130.0, event + std::chrono::seconds{1})), ErrorCategory::invalid_parameter);
    for (const auto offset : {std::chrono::seconds{1}, std::chrono::seconds{43200}}) {
        const auto context = market(130.0, event + offset);
        const double value = 2.0 * std::exp(-0.03 * detail::actual_365_fixed_year_fraction(event + offset, expiry));
        CHECK_THAT(*engine.price(fixed, context), Catch::Matchers::WithinAbs(value, 1e-12));
        error_is(implied_volatility(engine, fixed, context, value), ErrorCategory::unsupported_operation);
    }
    CHECK(*engine.price(*fixed.replace(BarrierTouchState::touched), market(100.0, event + std::chrono::seconds{1})) == 0.0);
    const auto weekend = test::day(2025, 1, 4);
    const auto bad_calendar = *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0, ObservationMode::scheduled, {weekend});
    const auto context = *make_pricing_context(market().model_parameters(), 130.0, start, weekdays_calendar());
    error_is(engine.price(bad_calendar, context), ErrorCategory::invalid_date);
}

TEST_CASE("Double touch Greeks match price differences and IV retains solver conventions", "[double-touch]")
{
    const auto engine = *make_finite_difference_double_touch_engine({800, 1600});
    for (const auto& original : {*make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0),
                                 *make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0, SettlementTiming::at_hit),
                                 *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0)}) {
        const auto option = *original.replace(BarrierTouchState::untouched);
        const auto result = engine.price_with_greeks(option, market(), true);
        REQUIRE(result);
        const auto bumped = test::price_differences(engine, option, market());
        REQUIRE(bumped);
        for (const auto measure : {Greek::delta, Greek::gamma, Greek::vega, Greek::rho})
            CHECK_THAT(test::greek_value(*result, measure), Catch::Matchers::WithinAbs(test::greek_value(*bumped, measure), 1e-5));
        for (const auto measure : {Greek::theta, Greek::charm, Greek::color, Greek::speed, Greek::vanna, Greek::zomma})
            CHECK(result->has(measure));
        const auto solved = implied_volatility(engine, option, market(), result->price(), {.lower_bound = 0.15, .upper_bound = 0.25});
        REQUIRE(solved);
        CHECK_THAT(*solved, Catch::Matchers::WithinAbs(0.2, 1e-6));
        error_is(implied_volatility(engine, option, market(), 2.0, {.lower_bound = 0.15, .upper_bound = 0.25}), ErrorCategory::unbracketed_volatility);
        for (const double spot : {80.0, 120.0}) {
            const auto boundary = engine.price_with_greeks(option, market(spot), true);
            REQUIRE(boundary);
            for (std::size_t index = 0; index < greek_count; ++index)
                CHECK_FALSE(boundary->has(static_cast<Greek>(index)));
        }
    }
    const auto scheduled = *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0, ObservationMode::scheduled, {first, expiry}, BarrierTouchState::untouched);
    const auto at_event = engine.price_with_greeks(scheduled, market(100.0, start_of_day(first)), {Greek::theta, Greek::charm, Greek::color});
    REQUIRE(at_event);
    CHECK_FALSE(at_event->has(Greek::theta));
    CHECK_FALSE(at_event->has(Greek::charm));
    CHECK_FALSE(at_event->has(Greek::color));
}

TEST_CASE("Double touch near barrier Greeks converge to independent series", "[double-touch][double-touch-greeks]")
{
    const std::array options{
        *make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0)->replace(BarrierTouchState::untouched),
        *make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0, SettlementTiming::at_hit)->replace(BarrierTouchState::untouched),
        *make_cash_double_no_touch(start, expiry, 80.0, 120.0, 1.0)->replace(BarrierTouchState::untouched)};
    // Separate absolute budgets in the library's per-unit, per-percent and per-day Greek units.
    const std::array budgets{
        std::pair{Greek::delta, 1e-7}, std::pair{Greek::gamma, 1e-7}, std::pair{Greek::speed, 1e-5},
        std::pair{Greek::theta, 1e-8}, std::pair{Greek::charm, 1e-8}, std::pair{Greek::color, 1e-8},
        std::pair{Greek::vega, 1e-8}, std::pair{Greek::vanna, 1e-7}, std::pair{Greek::zomma, 1e-6}, std::pair{Greek::rho, 1e-8}};
    const auto coarse = *make_finite_difference_double_touch_engine({800, 200});
    const auto fine = *make_finite_difference_double_touch_engine({25600, 1600});
    for (const double spot : {80.02, 119.98}) {
        const auto expected = reference_greeks(spot);
        const auto more_terms = cash_reference(spot, 0.2, 0.03, 1.0, 128);
        const auto integrated = continuous_reference(spot, 16384);
        for (std::size_t index = 0; index < options.size(); ++index) {
            CAPTURE(spot, index);
            CHECK_THAT(expected[index].price(), Catch::Matchers::WithinAbs(integrated[index], 1e-9));
            CHECK_THAT(test::greek_value(expected[index], Greek::gamma), Catch::Matchers::WithinAbs(more_terms[index].gamma, 1e-14));
            CHECK_THAT(test::greek_value(expected[index], Greek::speed), Catch::Matchers::WithinAbs(more_terms[index].speed, 1e-14));
            const auto rough = coarse.price_with_greeks(options[index], market(spot), true);
            const auto refined = fine.price_with_greeks(options[index], market(spot), true);
            REQUIRE(rough);
            REQUIRE(refined);
            CHECK_THAT(refined->price(), Catch::Matchers::WithinAbs(expected[index].price(), 1e-7));
            for (const auto& [measure, budget] : budgets) {
                CAPTURE(measure, budget);
                const double reference = test::greek_value(expected[index], measure);
                const double rough_error = std::abs(test::greek_value(*rough, measure) - reference);
                const double refined_error = std::abs(test::greek_value(*refined, measure) - reference);
                CHECK(refined_error < rough_error);
                CHECK(refined_error < budget);
                if (measure == Greek::gamma || measure == Greek::speed) CHECK(rough_error > budget);
            }
        }
    }
}

TEST_CASE("Double touch narrow grids low volatility and concurrent calls remain valid", "[double-touch]")
{
    const auto engine = *make_finite_difference_double_touch_engine({160, 800});
    const auto no_touch = *make_cash_double_no_touch(start, expiry, 99.5, 100.5, 1.0);
    const auto exited = engine.price(no_touch, market());
    REQUIRE(exited);
    CHECK(*exited < 1e-8);
    const auto stationary = engine.price(no_touch, market(100.0, start_of_day(start), 1e-12, 0.03, 0.03));
    REQUIRE(stationary);
    CHECK_THAT(*stationary, Catch::Matchers::WithinAbs(std::exp(-0.03), 1e-8));
    const auto unstable = *make_finite_difference_double_touch_engine({400, 1, FiniteDifferenceScheme::explicit_euler});
    error_is(unstable.price(no_touch, market()), ErrorCategory::invalid_parameter);
    error_is(make_finite_difference_double_touch_engine({2, 1}), ErrorCategory::invalid_parameter);
    const auto option = *make_cash_double_touch(start, expiry, 80.0, 120.0, 1.0);
    auto task = std::async(std::launch::async, [&] { return engine.price(option, market()); });
    const auto main = engine.price(option, market());
    const auto threaded = task.get();
    REQUIRE(main);
    REQUIRE(threaded);
    CHECK(*main == *threaded);
}
