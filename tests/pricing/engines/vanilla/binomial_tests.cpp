#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::greek_value;

TEST_CASE("Binomial immediate exercise returns payoff Greeks", "[binomial-exercise-greeks]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const auto type : {OptionType::put, OptionType::call}) {
        const bool put = type == OptionType::put;
        const double spot = put ? 88.0 : 100.0;
        const auto parameters = *make_bsm_parameters(put ? 0.15 : 0.0, put ? 0.0 : 0.15, 0.2);
        const auto option = *make_american_option(type, put ? 100.0 : 88.0, start, end);
        const auto context = *make_pricing_context(parameters, spot, start);
        for (const int steps : {1, 2, 10, 256}) {
            CAPTURE(type, steps);
            const auto engine = make_cox_ross_rubinstein_vanilla_engine({steps}).value();
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == 12.0);
            CHECK(greek_value(*result, Greek::delta) == (put ? -1.0 : 1.0));
            CHECK(greek_value(*result, Greek::gamma) == 0.0);
            const auto high = engine.price(option, *make_pricing_context(parameters, spot + 0.001, start));
            const auto low = engine.price(option, *make_pricing_context(parameters, spot - 0.001, start));
            REQUIRE(high);
            REQUIRE(low);
            CHECK_THAT((*high - *low) / 0.002,
                       Catch::Matchers::WithinAbs(greek_value(*result, Greek::delta), 1e-10));
        }
    }
}

TEST_CASE("Binomial speed tracks analytic sensitivity with default shifts", "[binomial-speed]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto engine = make_cox_ross_rubinstein_vanilla_engine({1000}).value();
    for (const double dividend : {0.0, 0.01}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, dividend, 0.2), 100.0,
                                                   start_of_day(start) + std::chrono::hours{12});
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(dividend, type);
            const auto european = *make_european_option(type, 100.0, start, end);
            const auto expected = AnalyticVanillaEngine{}.price_with_greeks(european, context, {Greek::speed});
            REQUIRE(expected);
            const auto check = [&](const auto& option) {
                const auto actual = engine.price_with_greeks(option, context, {Greek::speed});
                const auto price = engine.price(option, context);
                REQUIRE(actual);
                REQUIRE(price);
                CHECK(actual->price() == *price);
                const double reference = greek_value(*expected, Greek::speed);
                CHECK_THAT(greek_value(*actual, Greek::speed),
                           Catch::Matchers::WithinAbs(reference, std::abs(reference) * 0.02));
                CHECK_FALSE(actual->has(Greek::gamma));
            };
            check(european);
            if (type == OptionType::call && dividend == 0.0)
                check(*make_american_option(type, 100.0, start, end));
        }
    }
}

TEST_CASE("Binomial mixed Greeks track analytic sensitivities", "[binomial-mixed-greeks]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const GreeksRequest request{Greek::vanna, Greek::zomma, Greek::charm, Greek::color};
    const auto engine = make_cox_ross_rubinstein_vanilla_engine({1000}).value();
    for (const double dividend : {0.0, 0.01}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, dividend, 0.2), 100.0,
                                                   start_of_day(start) + std::chrono::hours{12});
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(dividend, type);
            const auto european = *make_european_option(type, 100.0, start, end);
            const auto expected = AnalyticVanillaEngine{}.price_with_greeks(european, context, request);
            REQUIRE(expected);
            const auto check = [&](const auto& option) {
                const auto actual = engine.price_with_greeks(option, context, request);
                const auto price = engine.price(option, context);
                REQUIRE(actual);
                REQUIRE(price);
                CHECK(actual->price() == *price);
                for (const auto greek : {Greek::vanna, Greek::zomma, Greek::charm, Greek::color}) {
                    CAPTURE(greek);
                    const double reference = greek_value(*expected, greek);
                    CHECK_THAT(greek_value(*actual, greek),
                               Catch::Matchers::WithinAbs(reference, std::abs(reference) * 0.02 + 1e-7));
                }
                CHECK_FALSE(actual->has(Greek::delta));
                CHECK_FALSE(actual->has(Greek::gamma));
            };
            check(european);
            if (type == OptionType::call && dividend == 0.0)
                check(*make_american_option(type, 100.0, start, end));
        }
    }
}

TEST_CASE("Binomial rejects underflowed asset trees")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto engine = kiyosi::make_cox_ross_rubinstein_vanilla_engine({1000}).value();
    for (const double volatility : {23.0, 25.0}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.05, 0.02, volatility), 100.0, start);
        for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
            const auto european = *kiyosi::make_european_option(type, 100.0, start, end);
            const auto american = *kiyosi::make_american_option(type, 100.0, start, end);
            for (const auto& price : {engine.price(european, context), engine.price(american, context)}) {
                REQUIRE_FALSE(price);
                CHECK(price.error().category == kiyosi::ErrorCategory::invalid_result);
            }
        }
    }
}

TEST_CASE("Binomial American engine prices expiry_date and validates steps")
{
    const auto expiry_date = day(2025, 1, 1);
    const auto option = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, expiry_date, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 90.0, expiry_date);
    const kiyosi::CoxRossRubinsteinVanillaEngine engine;

    const auto priced = engine.price(option, context);
    REQUIRE(priced.has_value());
    CHECK(*priced == 10.0);

    const auto invalid = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{0});
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto too_many = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{1'000'001});
    REQUIRE_FALSE(too_many.has_value());
    CHECK(too_many.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Binomial American engine supplements gamma below two steps")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto option = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);

    const auto result = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{1}).value().price_with_greeks(option, context, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
    REQUIRE(result.has_value());
    CHECK(std::isfinite(result->price()));
    CHECK(result->has(kiyosi::Greek::delta));
    CHECK(result->has(kiyosi::Greek::gamma));
    const auto gamma = result->get(kiyosi::Greek::gamma);
    REQUIRE(gamma.has_value());
    REQUIRE(gamma->has_value());
    CHECK_THAT(gamma.value().value_or(std::numeric_limits<double>::quiet_NaN()),
               Catch::Matchers::WithinAbs(0.0, 1e-7));
}

TEST_CASE("Binomial American engine exercises puts and converges to European calls")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 90.0, valuation);
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto european_put_option = *kiyosi::make_european_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american_call_option = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{400}).value();

    const auto american_put = engine.price(put, context);
    const auto european_put = kiyosi::AnalyticVanillaEngine{}.price(european_put_option, context);
    REQUIRE(american_put.has_value());
    REQUIRE(european_put.has_value());
    CHECK(*american_put >
          *european_put);

    const auto at_the_money_context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto american_call = engine.price_with_greeks(american_call_option, at_the_money_context, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
    const auto european_call = kiyosi::AnalyticVanillaEngine{}.price(call, at_the_money_context);
    REQUIRE(american_call.has_value());
    REQUIRE(european_call.has_value());
    CHECK_THAT(american_call->price(),
               WithinAbs(*european_call, 0.02));
    CHECK(std::isfinite(greek_value(*american_call, kiyosi::Greek::delta)));
    CHECK(std::isfinite(greek_value(*american_call, kiyosi::Greek::gamma)));

    const auto high_resolution = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{1200}).value().price(american_call_option, at_the_money_context);
    REQUIRE(high_resolution.has_value());
    CHECK_THAT(*high_resolution,
               WithinAbs(*european_call, 0.01));
}

TEST_CASE("Binomial American call and put values are symmetric at zero carry")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_american_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{200}).value();

    const auto call_result = engine.price(call, context);
    const auto put_result = engine.price(put, context);
    REQUIRE(call_result.has_value());
    REQUIRE(put_result.has_value());
    CHECK(std::abs(*call_result -
                   *put_result) <= 1e-12);
}

} // namespace
