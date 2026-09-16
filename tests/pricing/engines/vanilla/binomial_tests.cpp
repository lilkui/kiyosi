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
using kiyosi::test::risk_value;

TEST_CASE("Binomial American engine prices expiry and validates steps")
{
    const auto expiry = day(2025, 1, 1);
    const auto option = *kiyosi::make_american_option(
        kiyosi::option_type::put, 100.0, expiry, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 90.0, expiry);
    const kiyosi::CrrVanillaEngine engine;

    const auto priced = engine.price(option, context);
    REQUIRE(priced.has_value());
    CHECK(risk_value(*priced, kiyosi::risk_measure::price) == 10.0);

    const auto invalid = kiyosi::CrrVanillaEngine{kiyosi::BinomialSettings{0}}.price(option, context);
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::error_category::invalid_parameter);
    const auto too_many = kiyosi::CrrVanillaEngine{kiyosi::BinomialSettings{1'000'001}}
                              .price(option, context);
    REQUIRE_FALSE(too_many.has_value());
    CHECK(too_many.error().category == kiyosi::error_category::invalid_parameter);
}

TEST_CASE("Binomial American engine does not expose gamma below two steps")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto option = *kiyosi::make_american_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);

    const auto result = kiyosi::CrrVanillaEngine{kiyosi::BinomialSettings{1}}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(result->has(kiyosi::risk_measure::price));
    CHECK(result->has(kiyosi::risk_measure::delta));
    CHECK_FALSE(result->has(kiyosi::risk_measure::gamma));
    const auto gamma = result->get(kiyosi::risk_measure::gamma);
    REQUIRE(gamma.has_value());
    CHECK_FALSE(gamma->has_value());
}

TEST_CASE("Binomial American engine exercises puts and converges to European calls")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 90.0, valuation);
    const auto put = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, valuation, expiry);
    const auto european_put_option = *kiyosi::make_european_option(
        kiyosi::option_type::put, 100.0, valuation, expiry);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto american_call_option = *kiyosi::make_american_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const kiyosi::CrrVanillaEngine engine{kiyosi::BinomialSettings{400}};

    const auto american_put = engine.price(put, context);
    const auto european_put = kiyosi::AnalyticVanillaEngine{}.price(european_put_option, context);
    REQUIRE(american_put.has_value());
    REQUIRE(european_put.has_value());
    CHECK(risk_value(*american_put, kiyosi::risk_measure::price) >
          risk_value(*european_put, kiyosi::risk_measure::price));

    const auto at_the_money_context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto american_call = engine.price(american_call_option, at_the_money_context);
    const auto european_call = kiyosi::AnalyticVanillaEngine{}.price(call, at_the_money_context);
    REQUIRE(american_call.has_value());
    REQUIRE(european_call.has_value());
    CHECK_THAT(risk_value(*american_call, kiyosi::risk_measure::price),
               WithinAbs(risk_value(*european_call, kiyosi::risk_measure::price), 0.02));
    CHECK(std::isfinite(risk_value(*american_call, kiyosi::risk_measure::delta)));
    CHECK(std::isfinite(risk_value(*american_call, kiyosi::risk_measure::gamma)));

    const auto high_resolution = kiyosi::CrrVanillaEngine{kiyosi::BinomialSettings{1200}}
                                     .price(american_call_option, at_the_money_context);
    REQUIRE(high_resolution.has_value());
    CHECK_THAT(risk_value(*high_resolution, kiyosi::risk_measure::price),
               WithinAbs(risk_value(*european_call, kiyosi::risk_measure::price), 0.01));
}

TEST_CASE("Binomial American call and put values are symmetric at zero carry")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_american_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto put = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, valuation, expiry);
    const kiyosi::CrrVanillaEngine engine{kiyosi::BinomialSettings{200}};

    const auto call_result = engine.price(call, context);
    const auto put_result = engine.price(put, context);
    REQUIRE(call_result.has_value());
    REQUIRE(put_result.has_value());
    CHECK(std::abs(risk_value(*call_result, kiyosi::risk_measure::price) -
                   risk_value(*put_result, kiyosi::risk_measure::price)) <= 1e-12);
}

} // namespace
