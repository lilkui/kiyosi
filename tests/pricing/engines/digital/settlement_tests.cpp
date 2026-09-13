#include <catch2/catch_test_macros.hpp>

#include <array>

#include "support/reference_harness.hpp"

using kiyosi::test::measures;

TEST_CASE("Digital expiry settlement uses strict strikes without smooth Greeks")
{
    struct Settlement {
        kiyosi::option_type type;
        double spot;
        double cash;
        double asset;
    };
    // These exact settlement values are also checked against QuantLib payoff bindings.
    const std::array cases{
        Settlement{kiyosi::option_type::call, 99, 0, 0}, Settlement{kiyosi::option_type::call, 100, 0, 0},
        Settlement{kiyosi::option_type::call, 101, 10, 101}, Settlement{kiyosi::option_type::put, 99, 10, 99},
        Settlement{kiyosi::option_type::put, 100, 0, 0}, Settlement{kiyosi::option_type::put, 101, 0, 0}};
    const auto expiry = kiyosi::test::standard_expiry();
    for (const auto& item : cases) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), item.spot, expiry);
        const auto cash = *kiyosi::make_cash_or_nothing_option(item.type, 100, 10, expiry, expiry);
        const auto asset = *kiyosi::make_asset_or_nothing_option(item.type, 100, expiry, expiry);
        const auto check = [&](const auto& engine, const auto& option, double expected) {
            const auto result = engine.price(option, context);
            REQUIRE(result.has_value());
            REQUIRE(result->get(kiyosi::risk_measure::price).has_value());
            CHECK(*result->get(kiyosi::risk_measure::price) == expected);
            for (const auto& [name, measure] : measures)
                if (name != "price") CHECK_FALSE(result->get(measure).has_value());
        };
        const auto check_engine = [&](const auto& engine) {
            check(engine, cash, item.cash);
            check(engine, asset, item.asset);
        };
        check_engine(kiyosi::AnalyticDigitalEngine{});
        check_engine(kiyosi::IntegralDigitalEngine{});
        check_engine(kiyosi::FiniteDifferenceDigitalEngine{});
    }
}
