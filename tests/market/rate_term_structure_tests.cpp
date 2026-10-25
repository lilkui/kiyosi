#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <limits>
#include <numbers>
#include <ratio>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;
using kiyosi::test::greek_value;

constexpr Timestamp reference = Date{std::chrono::year{2025} / 1 / 1};
constexpr Timestamp middle = reference + std::chrono::days{180};
constexpr Timestamp maturity = reference + std::chrono::days{365};

RateTermStructure example_curve()
{
    const double first = std::exp(-0.02 * 180.0 / 365.0);
    const auto curve = make_rate_term_structure(reference, {middle, maturity},
                                                {first, first * std::exp(-0.08 * 185.0 / 365.0)});
    REQUIRE(curve);
    return *curve;
}

TEST_CASE("Rate curves interpolate log factors and interval forwards", "[term-structure]")
{
    const auto curve = example_curve();
    CHECK_FALSE(curve.is_flat());
    CHECK_FALSE(curve.flat_rate());
    CHECK(curve.reference_time() == reference);
    CHECK(curve.max_time() == maturity);
    CHECK(curve.nodes() == std::vector<Timestamp>{middle, maturity});
    REQUIRE(curve.factors().size() == 2);
    CHECK(curve.factors()[0] == std::exp(-0.02 * 180.0 / 365.0));
    const auto intraday = reference + std::chrono::hours{2172};
    const auto value = curve.factor(reference, intraday);
    REQUIRE(value);
    CHECK_THAT(*value, Catch::Matchers::WithinRel(std::exp(-0.02 * 90.5 / 365.0), 1e-13));
    const auto crossing = curve.log_factor(reference + std::chrono::days{90}, middle + std::chrono::days{30});
    REQUIRE(crossing);
    CHECK_THAT(*crossing, Catch::Matchers::WithinAbs(-(0.02 * 90.0 + 0.08 * 30.0) / 365.0, 1e-14));
    const auto left = curve.forward_rate(middle - std::chrono::microseconds{1});
    const auto right = curve.forward_rate(middle);
    const auto final = curve.forward_rate(maturity);
    REQUIRE(left);
    REQUIRE(right);
    REQUIRE(final);
    CHECK_THAT(*left, Catch::Matchers::WithinAbs(0.02, 1e-13));
    CHECK_THAT(*right, Catch::Matchers::WithinAbs(0.08, 1e-13));
    CHECK_THAT(*final, Catch::Matchers::WithinAbs(0.08, 1e-13));
    REQUIRE(curve.factor(reference, reference));
    CHECK(*curve.factor(reference, reference) == 1.0);
    REQUIRE(curve.factor(maturity, maturity));
    CHECK(*curve.factor(maturity, maturity) == 1.0);
    const auto whole = curve.factor(reference, maturity);
    const auto first = curve.factor(reference, middle);
    const auto last = curve.factor(middle, maturity);
    REQUIRE(whole);
    REQUIRE(first);
    REQUIRE(last);
    CHECK_THAT(*whole, Catch::Matchers::WithinRel(*first * *last, 1e-14));
}

TEST_CASE("Rate curves reject malformed nodes and out-of-coverage queries", "[term-structure]")
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double infinity = std::numeric_limits<double>::infinity();
    for (const double factor : {0.0, -0.5, nan, infinity}) {
        CAPTURE(factor);
        const auto invalid = make_rate_term_structure(reference, {middle}, {factor});
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    for (const auto& nodes : {std::vector<Timestamp>{}, std::vector<Timestamp>{reference},
                              std::vector<Timestamp>{reference - std::chrono::microseconds{1}},
                              std::vector<Timestamp>{middle, middle},
                              std::vector<Timestamp>{maturity, middle}}) {
        const auto invalid = make_rate_term_structure(reference, nodes, std::vector<double>(nodes.size(), 0.95));
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    const auto mismatched = make_rate_term_structure(reference, {middle, maturity}, {0.95});
    REQUIRE_FALSE(mismatched);
    CHECK(mismatched.error().category == ErrorCategory::invalid_parameter);
    const auto curve = example_curve();
    for (const auto time : {reference - std::chrono::microseconds{1}, maturity + std::chrono::microseconds{1}}) {
        const auto invalid_forward = curve.forward_rate(time);
        REQUIRE_FALSE(invalid_forward);
        CHECK(invalid_forward.error().category == ErrorCategory::invalid_time_range);
    }
    for (const auto interval : {std::array{reference - std::chrono::microseconds{1}, middle},
                                std::array{reference, maturity + std::chrono::microseconds{1}},
                                std::array{middle, reference}}) {
        const auto invalid = curve.factor(interval[0], interval[1]);
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_time_range);
    }
    const auto unsupported_date = start_of_day(Date{std::chrono::year::max() / 12 / 31} + std::chrono::days{1});
    const auto invalid_date = make_rate_term_structure(reference, {unsupported_date}, {0.9});
    REQUIRE_FALSE(invalid_date);
    CHECK(invalid_date.error().category == ErrorCategory::invalid_date);
    const auto invalid_reference = make_rate_term_structure(unsupported_date, {unsupported_date + std::chrono::days{1}}, {0.9});
    REQUIRE_FALSE(invalid_reference);
    CHECK(invalid_reference.error().category == ErrorCategory::invalid_date);
    const auto invalid_flat_reference = make_flat_rate(0.03, unsupported_date);
    REQUIRE_FALSE(invalid_flat_reference);
    CHECK(invalid_flat_reference.error().category == ErrorCategory::invalid_date);
    const auto signed_rates = make_rate_term_structure(reference, {middle, maturity}, {1.02, 0.99});
    REQUIRE(signed_rates);
    const auto negative_forward = signed_rates->forward_rate(reference);
    const auto positive_forward = signed_rates->forward_rate(middle);
    const auto signed_factor = signed_rates->factor(middle, maturity);
    REQUIRE(negative_forward);
    REQUIRE(positive_forward);
    REQUIRE(signed_factor);
    CHECK(*negative_forward < 0.0);
    CHECK(*positive_forward > 0.0);
    CHECK_THAT(*signed_factor, Catch::Matchers::WithinAbs(0.99 / 1.02, 1e-14));
}

TEST_CASE("Rate curves retain microsecond intervals far from their reference", "[term-structure]")
{
    const auto origin = start_of_day(day(1, 1, 1));
    const auto end = day(9998, 1, 1);
    const auto second = start_of_day(end);
    const auto first = second - std::chrono::microseconds{1};
    const auto curve = *make_rate_term_structure(origin, {first, second}, {std::exp(-100.0), std::exp(-101.0)});
    const auto log_factor = curve.log_factor(first, second);
    const auto factor = curve.factor(first, second);
    const auto left = curve.forward_rate(first - std::chrono::microseconds{1});
    const auto right = curve.forward_rate(first);
    REQUIRE(log_factor);
    REQUIRE(factor);
    REQUIRE(left);
    REQUIRE(right);
    CHECK_THAT(*log_factor, Catch::Matchers::WithinAbs(-1.0, 1e-14));
    CHECK_THAT(*factor, Catch::Matchers::WithinRel(std::exp(-1.0), 1e-14));
    const double long_years = std::chrono::duration<double, std::ratio<31'536'000>>{first - origin}.count();
    CHECK_THAT(*left, Catch::Matchers::WithinRel(100.0 / long_years, 1e-13));
    const double rate = 365.0 * 86400.0 * 1e6;
    CHECK_THAT(*right, Catch::Matchers::WithinRel(rate, 1e-13));
    const auto context = *make_pricing_context(*make_bsm_parameters(curve, *make_flat_rate(0), 0.2), 100, first);
    const auto equivalent = *make_pricing_context(*make_bsm_parameters(rate, 0, 0.2), 100, first);
    const auto option = *make_european_option(OptionType::call, 100, end - std::chrono::days{1}, end);
    const auto actual = AnalyticVanillaEngine{}.price(option, context);
    const auto expected = AnalyticVanillaEngine{}.price(option, equivalent);
    REQUIRE(actual);
    REQUIRE(expected);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(100 * (1 - std::exp(-1.0)), 1e-11));
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, 1e-11));
    const auto distant_context = *make_pricing_context(*make_bsm_parameters(curve, *make_flat_rate(0), 1e-8), 100, origin);
    const auto distant_option = *make_european_option(OptionType::call, std::exp(100.0), day(1, 1, 1), end);
    const auto unresolved = AnalyticVanillaEngine{}.price(distant_option, distant_context);
    REQUIRE_FALSE(unresolved);
    CHECK(unresolved.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Flat and shifted curves preserve their rate and coverage contracts", "[term-structure]")
{
    const auto flat = make_flat_rate(-0.03);
    const auto dated = make_flat_rate(-0.03, reference);
    REQUIRE(flat);
    REQUIRE(dated);
    CHECK(flat->is_flat());
    CHECK(flat->flat_rate() == -0.03);
    CHECK_FALSE(flat->reference_time());
    CHECK_FALSE(flat->max_time());
    CHECK(dated->reference_time() == reference);
    CHECK_FALSE(dated->max_time());
    const auto before = reference - std::chrono::days{365};
    const auto negative = flat->factor(before, reference);
    REQUIRE(negative);
    CHECK_THAT(*negative, Catch::Matchers::WithinRel(std::exp(0.03), 1e-14));
    const auto invalid = dated->factor(before, reference);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_time_range);
    const auto curve = example_curve();
    const auto shifted = curve.shifted(0.01);
    REQUIRE(shifted);
    CHECK(shifted->reference_time() == curve.reference_time());
    CHECK(shifted->max_time() == curve.max_time());
    CHECK(shifted->nodes() == curve.nodes());
    const auto unchanged = curve.shifted(0.0);
    REQUIRE(unchanged);
    CHECK(*unchanged == curve);
    const auto original = curve.factor(middle, maturity);
    const auto bumped = shifted->factor(middle, maturity);
    REQUIRE(original);
    REQUIRE(bumped);
    CHECK_THAT(*bumped, Catch::Matchers::WithinRel(*original * std::exp(-0.01 * 185.0 / 365.0), 1e-13));
    const auto flat_shift = flat->shifted(0.01);
    REQUIRE(flat_shift);
    CHECK_THAT(*flat_shift->flat_rate(), Catch::Matchers::WithinAbs(-0.02, 1e-15));
    const auto invalid_shift = curve.shifted(std::numeric_limits<double>::infinity());
    REQUIRE_FALSE(invalid_shift);
    CHECK(invalid_shift.error().category == ErrorCategory::invalid_parameter);
    for (const double rate : {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}) {
        const auto invalid_flat = make_flat_rate(rate);
        REQUIRE_FALSE(invalid_flat);
        CHECK(invalid_flat.error().category == ErrorCategory::invalid_parameter);
    }
}

TEST_CASE("Curve market contexts validate coverage without changing originals", "[term-structure]")
{
    const auto risk_free = example_curve();
    const auto dividend = make_rate_term_structure(reference - std::chrono::days{30},
                                                   {reference + std::chrono::days{90}, maturity + std::chrono::days{30}},
                                                   {0.99, 0.96});
    REQUIRE(dividend);
    const auto model = make_bsm_parameters(risk_free, *dividend, 0.2);
    REQUIRE(model);
    const auto scalar_rate = model->risk_free_rate();
    const auto scalar_yield = model->dividend_yield();
    REQUIRE_FALSE(scalar_rate);
    REQUIRE_FALSE(scalar_yield);
    CHECK(scalar_rate.error().category == ErrorCategory::unsupported_operation);
    CHECK(scalar_yield.error().category == ErrorCategory::unsupported_operation);
    const auto context = make_pricing_context(*model, 100, reference);
    REQUIRE(context);
    CHECK(context->model_parameters().risk_free_curve() == risk_free);
    CHECK(context->model_parameters().dividend_curve() == *dividend);
    const auto rolled = context->replace({.valuation_time = middle});
    REQUIRE(rolled);
    CHECK(rolled->valuation_time() == middle);
    CHECK(rolled->model_parameters() == *model);
    for (const auto time : {reference - std::chrono::microseconds{1}, maturity + std::chrono::microseconds{1}}) {
        const auto rejected = context->replace({.valuation_time = time});
        REQUIRE_FALSE(rejected);
        CHECK(rejected.error().category == ErrorCategory::invalid_time_range);
    }
    CHECK(context->valuation_time() == reference);
    CHECK(context->model_parameters() == *model);
    const auto replaced = model->replace({.volatility = 0.25});
    REQUIRE(replaced);
    CHECK(replaced->risk_free_curve() == risk_free);
    CHECK(replaced->dividend_curve() == *dividend);
    CHECK(replaced->volatility() == 0.25);
    const auto conflict = model->replace({.risk_free_rate = 0.04, .risk_free_curve = risk_free});
    REQUIRE_FALSE(conflict);
    CHECK(conflict.error().category == ErrorCategory::invalid_parameter);
    CHECK(model->volatility() == 0.2);
}

TEST_CASE("Analytic term-structure pricing preserves parity and parallel-shift rho", "[term-structure]")
{
    const auto risk_free = example_curve();
    const auto dividend = *make_rate_term_structure(reference - std::chrono::days{30},
                                                    {reference + std::chrono::days{90}, maturity},
                                                    {std::exp(-0.01 * 120.0 / 365.0), std::exp(-(0.01 * 120.0 + 0.03 * 275.0) / 365.0)});
    const auto model = *make_bsm_parameters(risk_free, dividend, 0.2);
    const auto context = *make_pricing_context(model, 100, reference);
    const auto call = *make_european_option(OptionType::call, 103, day(2025, 1, 1), day(2026, 1, 1));
    const auto put = *make_european_option(OptionType::put, 103, day(2025, 1, 1), day(2026, 1, 1));
    const AnalyticVanillaEngine engine;
    const auto call_price = engine.price(call, context);
    const auto put_price = engine.price(put, context);
    REQUIRE(call_price);
    REQUIRE(put_price);
    const double discount = std::exp(-(0.02 * 180.0 + 0.08 * 185.0) / 365.0);
    const double dividend_rate = (0.01 * 90.0 + 0.03 * 275.0) / 365.0;
    CHECK_THAT(*call_price - *put_price, Catch::Matchers::WithinAbs(100 * std::exp(-dividend_rate) - 103 * discount, 1e-11));
    const auto equivalent = *make_pricing_context(*make_bsm_parameters((0.02 * 180.0 + 0.08 * 185.0) / 365.0, dividend_rate, 0.2), 100, reference);
    const auto flat_price = engine.price(call, equivalent);
    REQUIRE(flat_price);
    CHECK_THAT(*call_price, Catch::Matchers::WithinAbs(*flat_price, 1e-11));
    const double h = 1e-5;
    const auto up_model = *make_bsm_parameters(*risk_free.shifted(h), dividend, 0.2);
    const auto down_model = *make_bsm_parameters(*risk_free.shifted(-h), dividend, 0.2);
    const auto up = engine.price(call, *context.replace({.model_parameters = up_model}));
    const auto down = engine.price(call, *context.replace({.model_parameters = down_model}));
    REQUIRE(up);
    REQUIRE(down);
    const auto result = engine.price_with_greeks(call, context, {Greek::rho});
    REQUIRE(result);
    CHECK_THAT(greek_value(*result, Greek::rho), Catch::Matchers::WithinAbs((*up - *down) / (200 * h), 1e-8));
}

TEST_CASE("Curve time Greeks follow the right-hand rate at knots", "[term-structure]")
{
    const auto later = middle + std::chrono::days{90};
    const auto risk = *make_rate_term_structure(reference, {middle, later, maturity},
                                                {std::exp(-0.02 * 180.0 / 365.0), std::exp(-(0.02 * 180.0 + 0.08 * 90.0) / 365.0), std::exp(-(0.02 * 180.0 + 0.08 * 90.0 - 0.03 * 95.0) / 365.0)});
    const auto dividend = *make_rate_term_structure(reference, {middle, later, maturity},
                                                    {std::exp(-0.01 * 180.0 / 365.0), std::exp(-(0.01 * 180.0 + 0.04 * 90.0) / 365.0), std::exp(-(0.01 * 180.0 + 0.04 * 90.0 - 0.02 * 95.0) / 365.0)});
    const auto context = *make_pricing_context(*make_bsm_parameters(risk, dividend, 0.2), 100, middle);
    const auto option = *make_european_option(OptionType::call, 100, day(2025, 1, 1), day(2026, 1, 1));
    const AnalyticVanillaEngine engine;
    const auto result = engine.price_with_greeks(option, context, {Greek::theta, Greek::charm, Greek::color});
    REQUIRE(result);
    const auto current = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
    const auto after = engine.price_with_greeks(option, *context.replace({.valuation_time = middle + std::chrono::seconds{60}}), {Greek::delta, Greek::gamma});
    REQUIRE(current);
    REQUIRE(after);
    CHECK_THAT(greek_value(*result, Greek::theta), Catch::Matchers::WithinAbs((after->price() - current->price()) * 1440, 2e-6));
    CHECK_THAT(greek_value(*result, Greek::charm), Catch::Matchers::WithinAbs((greek_value(*after, Greek::delta) - greek_value(*current, Greek::delta)) * 1440, 1e-7));
    CHECK_THAT(greek_value(*result, Greek::color), Catch::Matchers::WithinAbs((greek_value(*after, Greek::gamma) - greek_value(*current, Greek::gamma)) * 1440, 1e-8));
}

TEST_CASE("Flat-only engines reject node curves explicitly", "[term-structure]")
{
    const auto nodes = *make_rate_term_structure(reference, {maturity}, {std::exp(-0.03)});
    const auto context = *make_pricing_context(*make_bsm_parameters(nodes, *make_flat_rate(0.01), 0.2), 100, reference);
    const auto option = *make_european_option(OptionType::call, 100, day(2025, 1, 1), day(2026, 1, 1));
    const auto check = [&](const auto& engine) {
        const auto price = engine.price(option, context);
        REQUIRE_FALSE(price);
        CHECK(price.error().category == ErrorCategory::unsupported_operation);
        const auto result = engine.price_with_greeks(option, context, {Greek::delta});
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::unsupported_operation);
    };
    check(QuadratureVanillaEngine{});
    check(CoxRossRubinsteinVanillaEngine{});
}

TEST_CASE("Geometric Asian curve prices use weighted carry rather than terminal zero rates", "[term-structure]")
{
    const auto end = day(2025, 12, 31);
    const Timestamp expiry = start_of_day(end);
    const auto knot = reference + std::chrono::days{182};
    const double time = 364.0 / 365.0;
    const auto rates = *make_rate_term_structure(reference, {knot, expiry},
                                                 {std::exp(-0.02 * time / 2), std::exp(-0.05 * time)});
    const auto context = *make_pricing_context(*make_bsm_parameters(rates, *make_flat_rate(0), 0.2), 100, reference);
    const auto option = *make_geometric_average_option(OptionType::call, 100, day(2025, 1, 1), day(2025, 1, 1), end);
    const double variance = 0.04 * time / 3;
    const double drift = (0.02 * 3 / 8 + 0.08 / 8 - 0.04 / 4) * time;
    const double deviation = std::sqrt(variance);
    const double d2 = drift / deviation;
    const double d1 = d2 + deviation;
    const auto cdf = [](double z) { return 0.5 * std::erfc(-z / std::numbers::sqrt2); };
    const double expected = std::exp(-0.05 * time) * 100 *
                            (std::exp(drift + variance / 2) * cdf(d1) - cdf(d2));
    const auto price = AnalyticGeometricAveragePriceEngine{}.price(option, context);
    REQUIRE(price);
    CHECK_THAT(*price, Catch::Matchers::WithinAbs(expected, 1e-11));
}

TEST_CASE("Arithmetic Asian curves preserve average moments and seasoned affine payoffs", "[term-structure]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 12, 31);
    const Timestamp expiry = start_of_day(end);
    const double half = 182.0 / 365.0;
    const auto rates = *make_rate_term_structure(reference, {reference + std::chrono::days{182}, expiry},
                                                 {std::exp(-0.02 * half), std::exp(-0.10 * half)});
    const auto context = *make_pricing_context(*make_bsm_parameters(rates, *make_flat_rate(0), 0.2), 100, reference);
    const auto call = *make_arithmetic_average_option(OptionType::call, 100, start, start, end);
    const auto put = *make_arithmetic_average_option(OptionType::put, 100, start, start, end);
    const double mean = 100 * (std::expm1(0.02 * half) / 0.02 + std::exp(0.02 * half) * std::expm1(0.08 * half) / 0.08) /
                        (2 * half);
    const auto engine = *make_monte_carlo_asian_engine({.path_count = 65536, .step_count = 64, .seed = 7, .sampling = MonteCarloSampling::sobol, .antithetic = true});
    const auto call_price = engine.price(call, context);
    const auto put_price = engine.price(put, context);
    REQUIRE(call_price);
    REQUIRE(put_price);
    CHECK_THAT(*call_price - *put_price, Catch::Matchers::WithinAbs(std::exp(-0.10 * half) * (mean - 100), 0.01));

    const auto seasoned = *make_arithmetic_average_option(OptionType::call, 25, start - std::chrono::days{364}, start - std::chrono::days{364}, end, 100);
    const auto affine = engine.price(seasoned, context);
    REQUIRE(affine);
    CHECK_THAT(*affine, Catch::Matchers::WithinAbs(std::exp(-0.10 * half) * (25 + mean / 2), 1e-11));
    const auto seasoned_put = *make_arithmetic_average_option(OptionType::put, 25, start - std::chrono::days{364}, start - std::chrono::days{364}, end, 100);
    const auto zero = engine.price(seasoned_put, context);
    REQUIRE(zero);
    CHECK(*zero == 0.0);
    const auto unsupported = TurnbullWakemanArithmeticAveragePriceEngine{}.price(call, context);
    REQUIRE_FALSE(unsupported);
    CHECK(unsupported.error().category == ErrorCategory::unsupported_operation);
    const auto geometric = *make_geometric_average_option(OptionType::call, 100, start, start, end);
    const auto unsupported_geometric = engine.price(geometric, context);
    REQUIRE_FALSE(unsupported_geometric);
    CHECK(unsupported_geometric.error().category == ErrorCategory::unsupported_operation);
}
} // namespace
