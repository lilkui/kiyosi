#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <future>
#include <limits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing/detail/numerical_greeks.hpp"
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using test::day;
constexpr auto start = Date{std::chrono::year{2025} / 1 / 1};

RangeAccrualTerms terms(Date end = start + std::chrono::days{16})
{
    return {1'000, 0.12, 95, 110, start, end, {}};
}

PricingContext market(Timestamp valuation = start, double spot = 100.0, double rate = 0.04,
                      TradingCalendar calendar = all_days_calendar())
{
    const auto result = make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.2), spot, valuation, std::move(calendar));
    REQUIRE(result);
    return *result;
}

// Independent public-input oracle: linearity of expectation needs only lognormal marginals.
double reference(const RangeAccrualOption& option, const PricingContext& context)
{
    double hits = option.realized_hit_count().value_or(0);
    int total = 0;
    const auto& model = context.model_parameters();
    const auto time = context.valuation_time();
    for (auto date = option.effective_date() + std::chrono::days{1}; date <= option.expiry_date(); date += std::chrono::days{1}) {
        if (!context.calendar().is_trading_day(date)) continue;
        ++total;
        if (date < time) continue;
        if (date == time) {
            hits += option.lower_bound() <= context.spot_price() && context.spot_price() <= option.upper_bound() ? 1.0 : 0.0;
            continue;
        }
        const double t = std::chrono::duration<double>(date - time).count() / (365 * 86400);
        const double mean = std::log(context.spot_price()) - *model.risk_free_curve().log_factor(time, date) +
                            *model.dividend_curve().log_factor(time, date) - 0.5 * model.volatility() * model.volatility() * t;
        const double width = model.volatility() * std::sqrt(2 * t);
        const double lower = option.lower_bound() == 0.0 ? 0.0 : std::erfc((mean - std::log(option.lower_bound())) / width) / 2;
        const double upper = std::erfc((mean - std::log(option.upper_bound())) / width) / 2;
        hits += upper - lower;
    }
    return option.notional() * option.annual_coupon_rate() * static_cast<double>((option.expiry_date() - option.effective_date()).count()) / 365 *
           hits / total * *model.risk_free_curve().factor(time, option.expiry_date());
}

struct ReferenceEngine {
    Result<double> price(const RangeAccrualOption& option, const PricingContext& context) const { return reference(option, context); }
};

TEST_CASE("Range accrual equals the exact daily marginal sum", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms(start + std::chrono::days{365}));
    const auto context = market();
    const auto price = AnalyticRangeAccrualEngine{}.price(option, context);
    REQUIRE(price);
    CHECK(std::abs(*price - reference(option, context)) < 1e-10);
    const auto volatility = implied_volatility(AnalyticRangeAccrualEngine{}, option, context, *price,
                                               {.lower_bound = 0.1, .upper_bound = 0.4});
    REQUIRE(volatility);
    CHECK(std::abs(*volatility - context.model_parameters().volatility()) < 1e-8);
}

TEST_CASE("Range accrual validates terms and preserves values on replacement", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms());
    CHECK(option.notional() == 1'000);
    CHECK(option.annual_coupon_rate() == 0.12);
    CHECK(option.lower_bound() == 95);
    CHECK(option.upper_bound() == 110);
    CHECK(option.effective_date() == start);
    CHECK(option.expiry_date() == start + std::chrono::days{16});
    CHECK_FALSE(option.realized_hit_count());
    const auto replaced = option.replace(3);
    REQUIRE(replaced);
    CHECK(replaced->realized_hit_count() == 3);
    CHECK(*replaced->replace({}) == option);
    CHECK_FALSE(option.realized_hit_count());
    const auto rejected = option.replace(-1);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_parameter);
    CHECK(option == *make_range_accrual_option(terms()));
    for (const double bad : {-1.0, std::numeric_limits<double>::infinity(), std::numeric_limits<double>::quiet_NaN()}) {
        for (const int field : {0, 1, 2, 3}) {
            auto input = terms();
            if (field == 0) input.notional = bad;
            if (field == 1) input.annual_coupon_rate = bad;
            if (field == 2) input.lower_bound = bad;
            if (field == 3) input.upper_bound = bad;
            const auto result = make_range_accrual_option(input);
            REQUIRE_FALSE(result);
            CHECK(result.error().category == ErrorCategory::invalid_parameter);
        }
    }
    for (const auto input : {RangeAccrualTerms{0, 0.1, 90, 110, start, start + std::chrono::days{1}, {}},
                             RangeAccrualTerms{1, 0.1, 110, 110, start, start + std::chrono::days{1}, {}}})
        CHECK_FALSE(make_range_accrual_option(input));
    const auto zero_term = make_range_accrual_option(terms(start));
    REQUIRE_FALSE(zero_term);
    CHECK(zero_term.error().category == ErrorCategory::invalid_time_range);
    auto zero = terms();
    zero.lower_bound = 0;
    zero.annual_coupon_rate = 0;
    CHECK(make_range_accrual_option(zero));
}

TEST_CASE("Range accrual counts closed endpoints and adjacent prices exactly once at expiry", "[range-accrual]")
{
    auto input = terms(start + std::chrono::days{2});
    input.realized_hit_count = 1;
    const auto option = *make_range_accrual_option(input);
    const double unit = 1'000 * 0.12 / 365;
    const AnalyticRangeAccrualEngine engine;
    for (const auto [spot, hit] : std::array{
             std::pair{std::nextafter(95.0, 0.0), false}, std::pair{95.0, true},
             std::pair{std::nextafter(95.0, 100.0), true}, std::pair{std::nextafter(110.0, 100.0), true},
             std::pair{110.0, true}, std::pair{std::nextafter(110.0, 120.0), false}}) {
        CAPTURE(spot);
        const auto result = engine.price_with_greeks(option, market(input.expiry_date, spot), true);
        REQUIRE(result);
        CHECK(result->price() == Catch::Approx(unit * (hit ? 2 : 1)).margin(1e-12));
        for (std::size_t index = 0; index < greek_count; ++index)
            CHECK_FALSE(result->has(static_cast<Greek>(index)));
    }
}

TEST_CASE("Range accrual history distinguishes before start midnight intraday and expiry", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms());
    const auto engine = AnalyticRangeAccrualEngine{};
    for (const auto time : {start_of_day(start) - std::chrono::days{3}, start_of_day(start), start_of_day(start) + std::chrono::hours{12}}) {
        const auto result = engine.price(option, market(time));
        REQUIRE(result);
        CHECK(*result == Catch::Approx(reference(option, market(time))).margin(1e-10));
    }
    for (const auto time : {start_of_day(start) + std::chrono::days{1}, start_of_day(start) + std::chrono::days{1} + std::chrono::hours{12},
                            start_of_day(start) + std::chrono::days{8}}) {
        const int elapsed = static_cast<int>((time - start_of_day(start) - std::chrono::microseconds{1}) / std::chrono::days{1});
        const auto valid = *option.replace(elapsed);
        const auto result = engine.price(valid, market(time));
        REQUIRE(result);
        CHECK(*result == Catch::Approx(reference(valid, market(time))).margin(1e-10));
        const auto too_many = engine.price(*option.replace(elapsed + 1), market(time));
        REQUIRE_FALSE(too_many);
        CHECK(too_many.error().category == ErrorCategory::invalid_parameter);
        if (elapsed != 0) {
            const auto missing = engine.price(option, market(time));
            REQUIRE_FALSE(missing);
            CHECK(missing.error().category == ErrorCategory::invalid_parameter);
        }
    }
    const auto after = engine.price(*option.replace(16), market(start_of_day(option.expiry_date()) + std::chrono::microseconds{1}));
    REQUIRE_FALSE(after);
    CHECK(after.error().category == ErrorCategory::invalid_time_range);
}

TEST_CASE("Range accrual preserves full-contract scaling and validates curves and calendars", "[range-accrual]")
{
    auto input = terms(day(2025, 1, 6));
    input.effective_date = day(2025, 1, 3);
    const auto option = *make_range_accrual_option(input);
    const auto context = market(day(2025, 1, 3), 100, 0.04, weekdays_calendar());
    const auto one = AnalyticRangeAccrualEngine{};
    const auto value = one.price(option, context);
    REQUIRE(value);
    CHECK(*value == Catch::Approx(reference(option, context)).margin(1e-12));
    const auto weekend = market(start_of_day(day(2025, 1, 4)) + std::chrono::hours{12}, 100, 0.04, weekdays_calendar());
    const auto weekend_greeks = one.price_with_greeks(option, weekend, {Greek::theta, Greek::charm, Greek::color});
    REQUIRE(weekend_greeks);
    CHECK(weekend_greeks->has(Greek::theta));
    CHECK(weekend_greeks->has(Greek::charm));
    CHECK(weekend_greeks->has(Greek::color));
    const auto invalid = one.price(*make_range_accrual_option(terms(day(2025, 1, 5))), context);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_date);
    const auto short_curve = *make_rate_term_structure(start, {start + std::chrono::days{1}}, {0.999});
    const auto short_context = *make_pricing_context(*make_bsm_parameters(short_curve, 0.01, 0.2), 100, start);
    const auto uncovered = one.price(*make_range_accrual_option(terms()), short_context);
    REQUIRE_FALSE(uncovered);
    CHECK(uncovered.error().category == ErrorCategory::invalid_time_range);
    auto zero = terms();
    zero.annual_coupon_rate = 0;
    CHECK(*one.price(*make_range_accrual_option(zero), short_context) == 0);
    const auto missing = one.price(*make_range_accrual_option(zero), market(start + std::chrono::days{3}));
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("Range accrual discounts the entire equal-weighted coupon only at expiry", "[range-accrual]")
{
    const auto engine = AnalyticRangeAccrualEngine{};
    const auto model = *make_bsm_parameters(0.8, 0.8, 1e-9);
    const auto option = *make_range_accrual_option(terms());
    const auto initial = *make_pricing_context(model, 100, start, all_days_calendar());
    CHECK(*engine.price(option, initial) == Catch::Approx(120 * 16.0 / 365 * std::exp(-0.8 * 16 / 365)).margin(1e-12));
    const auto live = *option.replace(3);
    const auto current = *make_pricing_context(model, 100, start + std::chrono::days{7}, all_days_calendar());
    CHECK(*engine.price(live, current) == Catch::Approx(120 * 13.0 / 365 * std::exp(-0.8 * 9 / 365)).margin(1e-12));
    const auto outside = *make_pricing_context(model, 80, start, all_days_calendar());
    CHECK(*engine.price(option, outside) == 0);
}

TEST_CASE("Range accrual matches exact BSM probabilities for flat and nonflat rates", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms());
    const auto curve = *make_rate_term_structure(start, {start + std::chrono::days{5}, option.expiry_date()}, {1.001, 0.997});
    const auto dividend = *make_rate_term_structure(start - std::chrono::days{1}, {start + std::chrono::days{7}, option.expiry_date()}, {0.998, 1.002});
    for (const auto& context : {market(), market(start, 100, -0.03), *make_pricing_context(*make_bsm_parameters(curve, dividend, 0.2), 100, start)}) {
        const auto price = AnalyticRangeAccrualEngine{}.price(option, context);
        REQUIRE(price);
        CHECK(std::abs(*price - reference(option, context)) < 1e-10);
    }
}

TEST_CASE("Range accrual combines tiny probabilities and large coupons without losing range", "[range-accrual]")
{
    const double mean = std::log(100.0) + 0.01 / 365;
    const double width = 0.2 / std::sqrt(365.0);
    auto input = terms(start + std::chrono::days{1});
    input.notional = 1e300;
    input.annual_coupon_rate = 1e100;
    input.lower_bound = std::exp(mean + 40 * width);
    input.upper_bound = std::exp(mean + 41 * width);
    const auto option = *make_range_accrual_option(input);
    const auto engine = AnalyticRangeAccrualEngine{};
    const auto value = engine.price(option, market());
    REQUIRE(value);
    // log P(Z > 40) = -804.608442013753788; the Z > 41 tail is negligible at double precision.
    const double expected = std::exp(std::log(1e300) + std::log(1e100) - std::log(365.0) - 0.04 / 365 - 804.608442013753788);
    CHECK(*value == Catch::Approx(expected).epsilon(2e-11));
    input.lower_bound = 95;
    input.upper_bound = 110;
    const auto overflow = engine.price(*make_range_accrual_option(input), market());
    REQUIRE_FALSE(overflow);
    CHECK(overflow.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Range accrual retains adjacent price bounds at tiny volatility", "[range-accrual]")
{
    const double below = std::nextafter(100.0, 0.0);
    const double above = std::nextafter(100.0, std::numeric_limits<double>::infinity());
    struct Scenario {
        double lower, upper, volatility, probability;
        int days;
    };
    for (const auto scenario : std::array{
             Scenario{100, above, 1e-16, 0.5, 1}, Scenario{below, 100, 1e-16, 0.5, 1},
             Scenario{below, above, 1e-16, 1.0, 1}, Scenario{100, above, 1e-16, 0.5, 2},
             Scenario{100, above, std::numeric_limits<double>::denorm_min(), 0.5, 1},
             Scenario{below, 100, std::numeric_limits<double>::denorm_min(), 0.5, 1},
             Scenario{below, above, std::numeric_limits<double>::denorm_min(), 1.0, 1},
             Scenario{below, 110, 1e-14, 0.6069959804126283, 1}, Scenario{90, above, 1e-14, 0.6069959804126285, 1}}) {
        CAPTURE(scenario.lower, scenario.upper, scenario.volatility, scenario.days);
        const auto option = *make_range_accrual_option({365.0 / scenario.days, 1, scenario.lower, scenario.upper, start, start + std::chrono::days{scenario.days}, {}});
        const auto context = *make_pricing_context(*make_bsm_parameters(0, 0, scenario.volatility), 100, start, all_days_calendar());
        const auto engine = AnalyticRangeAccrualEngine{};
        const auto price = engine.price(option, context);
        REQUIRE(price);
        CHECK(*price == Catch::Approx(scenario.probability).margin(2e-14));
    }
}

TEST_CASE("Range accrual exact prices support shared and distinct concurrent engines", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms());
    const auto context = market();
    const AnalyticRangeAccrualEngine engine;
    const double expected = reference(option, context);
    auto concurrent = std::async(std::launch::async, [&] { return engine.price(option, context); });
    CHECK(std::abs(*engine.price(option, context) - expected) < 1e-10);
    CHECK(std::abs(*AnalyticRangeAccrualEngine{}.price(option, context) - expected) < 1e-10);
    CHECK(std::abs(*concurrent.get() - expected) < 1e-10);
}

TEST_CASE("Range accrual smooth Greeks match independent reference and honor event availability", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms(start + std::chrono::days{1}));
    const auto context = market(start_of_day(start) - std::chrono::days{3}, 99);
    const auto engine = AnalyticRangeAccrualEngine{};
    const auto result = engine.price_with_greeks(option, context, true, {.spot_shift = 0.1});
    REQUIRE(result);
    const auto expected = detail::price_with_greeks(ReferenceEngine{}, option, context, true, {.spot_shift = 0.1});
    REQUIRE(expected);
    for (std::size_t index = 0; index < greek_count; ++index) {
        const auto greek = static_cast<Greek>(index);
        CAPTURE(index);
        REQUIRE(result->has(greek));
        CHECK(*result->require(greek) == Catch::Approx(*expected->require(greek)).margin(2e-8));
    }
    CHECK(result->price() == *engine.price(option, context));
    auto current_terms = terms();
    current_terms.realized_hit_count = 0;
    const auto current = *make_range_accrual_option(current_terms);
    const auto at_bound = engine.price_with_greeks(current, market(start + std::chrono::days{1}, 95), true);
    REQUIRE(at_bound);
    for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed, Greek::vanna, Greek::zomma, Greek::theta, Greek::charm, Greek::color})
        CHECK_FALSE(at_bound->has(greek));
    CHECK(at_bound->has(Greek::vega));
    CHECK(at_bound->has(Greek::rho));
    const auto crossed = engine.price_with_greeks(current, market(start + std::chrono::days{1}, 95.005), {Greek::delta});
    REQUIRE(crossed);
    CHECK_FALSE(crossed->has(Greek::delta));
    const auto clear = engine.price_with_greeks(current, market(start + std::chrono::days{1}, 95.1), {Greek::delta});
    REQUIRE(clear);
    CHECK(clear->has(Greek::delta));
    const auto time_event = engine.price_with_greeks(current, market(start_of_day(start) + std::chrono::days{2} + std::chrono::hours{12}), {Greek::theta, Greek::charm, Greek::color});
    REQUIRE(time_event);
    CHECK_FALSE(time_event->has(Greek::theta));
    CHECK_FALSE(time_event->has(Greek::charm));
    CHECK_FALSE(time_event->has(Greek::color));
}

TEST_CASE("Range accrual multi-observation Greeks match independent curve reference", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms(start + std::chrono::days{8}));
    const auto curve = *make_rate_term_structure(start - std::chrono::days{5}, {start + std::chrono::days{3}, option.expiry_date()}, {1.001, 0.997});
    const auto context = *make_pricing_context(*make_bsm_parameters(curve, -0.01, 0.2), 99, start - std::chrono::days{3}, all_days_calendar());
    const NumericalShiftSettings shifts{.spot_shift = 0.1};
    const auto exact = detail::price_with_greeks(ReferenceEngine{}, option, context, true, shifts);
    const auto result = AnalyticRangeAccrualEngine{}.price_with_greeks(option, context, true, shifts);
    REQUIRE(exact);
    REQUIRE(result);
    for (std::size_t index = 0; index < greek_count; ++index) {
        const auto greek = static_cast<Greek>(index);
        CAPTURE(index);
        REQUIRE(result->has(greek));
        CHECK(std::abs(*result->require(greek) - *exact->require(greek)) < 2e-8);
    }
}

TEST_CASE("Range accrual supports long contracts without a sampling dimension limit", "[range-accrual]")
{
    const AnalyticRangeAccrualEngine engine;
    const auto option = *make_range_accrual_option(terms(start + std::chrono::days{21'204}));
    const auto price = engine.price(option, market());
    REQUIRE(price);
    CHECK(std::abs(*price - reference(option, market())) < 1e-8);
    const auto in_life = *option.replace(20'000);
    const auto context = market(start + std::chrono::days{20'001});
    const auto value = engine.price(in_life, context);
    REQUIRE(value);
    CHECK(std::abs(*value - reference(in_life, context)) < 1e-8);
}

TEST_CASE("Range accrual validates Greek requests and numerical shifts", "[range-accrual]")
{
    const auto option = *make_range_accrual_option(terms());
    const auto empty = AnalyticRangeAccrualEngine{}.price_with_greeks(option, market(), {});
    REQUIRE_FALSE(empty);
    CHECK(empty.error().category == ErrorCategory::invalid_parameter);
    const auto bad_shift = AnalyticRangeAccrualEngine{}.price_with_greeks(option, market(), {Greek::delta}, {.volatility_shift = 0});
    REQUIRE_FALSE(bad_shift);
    CHECK(bad_shift.error().category == ErrorCategory::invalid_parameter);
    const auto unavailable = AnalyticRangeAccrualEngine{}.price_with_greeks(option, market(), {Greek::delta}, {.spot_shift = 200});
    REQUIRE(unavailable);
    CHECK_FALSE(unavailable->has(Greek::delta));
}

} // namespace
