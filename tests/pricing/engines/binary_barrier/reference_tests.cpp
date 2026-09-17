#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <cmath>
#include <string>

#include "support/reference_harness.hpp"

using kiyosi::test::barrier_kinds;
using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("QuantLib binary barrier and touch contracts validate prices and smooth Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    int generated = 0, wrappers = 0, binaries = 0, touches = 0;
    for (const auto& fixture : cases) {
        if (fixture.instrument != "BinaryBarrierOption" && fixture.instrument != "TouchOption")
            continue;
        const auto& inputs = fixture.inputs;
        INFO("case=" << fixture.case_id);
        REQUIRE(inputs.at("owner") == "QuantLib");
        REQUIRE(fixture.engine == "AnalyticBinaryBarrierEngine");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBinaryBarrierEngine+QuantLib.AnalyticDigitalAmericanEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        REQUIRE(barrier_kinds.contains(inputs.at("barrier_kind")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put" ||
                 inputs.at("option") == "none"));
        REQUIRE((inputs.at("asset_settlement") == "true" ||
                 inputs.at("asset_settlement") == "false"));
        REQUIRE((inputs.at("settlement") == "at_hit" ||
                 inputs.at("settlement") == "at_expiry"));
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        const auto parameters = kiyosi::make_bsm_parameters(
            number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters);
        const auto context = kiyosi::make_pricing_context(
            *parameters, number("spot"), date("valuation"));
        REQUIRE(context);
        const kiyosi::AnalyticBinaryBarrierEngine engine;
        const bool boundary = number("spot") == number("barrier") ||
                              (date("expiry") - date("valuation")).count() <= 2;
        const auto check = [&](const auto& option) {
            const auto native = engine.price(option, *context);
            check_price(fixture, native);
            for (const auto& [name, measure] : measures)
                REQUIRE(native->has(measure) == (name == "price"));
            for (const auto& [name, value] : fixture.outputs)
                REQUIRE(measures.contains(name));
            ++generated;
            REQUIRE(fixture.case_id.starts_with("ql-binary-"));
            REQUIRE(inputs.at("wrapper") == (boundary ? "false" : "true"));
            if (boundary) return;
            ++wrappers;
            const kiyosi::NumericalShiftSettings shifts{
                number("spot_shift"), number("volatility_shift"), number("rate_shift"),
                static_cast<int>(number("time_shift_days"))};
            REQUIRE(std::abs(number("spot") - number("barrier")) > 3 * shifts.spot_shift);
            const auto numerical = kiyosi::NumericalAnalyticsEngine{engine, shifts}.price(
                option, *context);
            REQUIRE(numerical);
            for (const auto& [name, measure] : measures) {
                INFO("measure=" << name);
                REQUIRE(numerical->has(measure));
                CHECK_THAT(*numerical->require(measure),
                           Catch::Matchers::WithinAbs(
                               fixture.outputs.at(name),
                               number("numerical_tolerance_" + name) +
                                   number("uncertainty_" + name)));
            }
        };

        const bool asset = inputs.at("asset_settlement") == "true";
        if (inputs.at("option") != "none") {
            REQUIRE(fixture.instrument == "BinaryBarrierOption");
            const kiyosi::BinaryBarrierTerms terms{
                .type = inputs.at("option") == "call" ? kiyosi::option_type::call
                                                       : kiyosi::option_type::put,
                .strike = number("strike"),
                .effective = date("effective"),
                .expiry = date("expiry"),
                .barrier = number("barrier"),
                .barrier_kind = barrier_kinds.at(inputs.at("barrier_kind"))};
            if (asset) {
                const auto option = kiyosi::make_asset_binary_barrier_option(terms);
                REQUIRE(option);
                check(*option);
            } else {
                const auto option = kiyosi::make_cash_binary_barrier_option(
                    terms, number("payout"));
                REQUIRE(option);
                check(*option);
            }
            ++binaries;
            continue;
        }

        REQUIRE(fixture.instrument == "TouchOption");
        const auto effective = date("effective");
        const auto expiry = date("expiry");
        const double barrier = number("barrier");
        const bool up = inputs.at("barrier_kind").starts_with("up");
        const bool one_touch = inputs.at("barrier_kind").ends_with("in");
        const auto timing = inputs.at("settlement") == "at_hit"
                                ? kiyosi::settlement_timing::at_hit
                                : kiyosi::settlement_timing::at_expiry;
        if (asset) {
            const auto option = one_touch
                ? (up ? kiyosi::make_asset_one_touch_up(effective, expiry, barrier, timing)
                      : kiyosi::make_asset_one_touch_down(effective, expiry, barrier, timing))
                : (up ? kiyosi::make_asset_no_touch_up(effective, expiry, barrier)
                      : kiyosi::make_asset_no_touch_down(effective, expiry, barrier));
            REQUIRE(option);
            check(*option);
        } else {
            const auto option = one_touch
                ? (up ? kiyosi::make_cash_one_touch_up(
                            effective, expiry, barrier, number("payout"), timing)
                      : kiyosi::make_cash_one_touch_down(
                            effective, expiry, barrier, number("payout"), timing))
                : (up ? kiyosi::make_cash_no_touch_up(
                            effective, expiry, barrier, number("payout"))
                      : kiyosi::make_cash_no_touch_down(
                            effective, expiry, barrier, number("payout")));
            REQUIRE(option);
            check(*option);
        }
        ++touches;
    }
    CHECK(generated == 268);
    CHECK(binaries == 160);
    CHECK(touches == 108);
    CHECK(wrappers == 140);
}
