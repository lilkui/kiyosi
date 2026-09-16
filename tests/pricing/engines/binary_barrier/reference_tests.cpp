#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <cmath>
#include <optional>
#include <string>

#include "support/reference_harness.hpp"

using kiyosi::test::barrier_kinds;
using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("QuantLib binary barrier contracts validate prices and smooth Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    int generated = 0, wrappers = 0;
    for (const auto& fixture : cases) {
        const auto& inputs = fixture.inputs;
        if (fixture.instrument != "BinaryBarrierOption") continue;
        INFO("case=" << fixture.case_id);
        REQUIRE(inputs.at("owner") == "QuantLib");
        REQUIRE(fixture.engine == "AnalyticBinaryBarrierEngine");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBinaryBarrierEngine+QuantLib.AnalyticDigitalAmericanEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        REQUIRE(barrier_kinds.contains(inputs.at("barrier_kind")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put" || inputs.at("option") == "none"));
        REQUIRE((inputs.at("asset_settlement") == "true" || inputs.at("asset_settlement") == "false"));
        REQUIRE((inputs.at("settlement") == "at_hit" || inputs.at("settlement") == "at_expiry"));
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        std::optional<kiyosi::option_type> type;
        if (inputs.at("option") != "none")
            type = inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put;
        const auto option = kiyosi::make_binary_barrier_option({
            .type = type,
            .strike = number("strike"),
            .effective = date("effective"),
            .expiry = date("expiry"),
            .barrier = number("barrier"),
            .barrier_kind = barrier_kinds.at(inputs.at("barrier_kind")),
            .payout = number("payout"),
            .asset_settlement = inputs.at("asset_settlement") == "true",
            .settlement_timing = inputs.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit
                                                                      : kiyosi::rebate_timing::at_expiry});
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, number("spot"), date("valuation"));
        REQUIRE(context.has_value());
        const kiyosi::AnalyticBinaryBarrierEngine engine;
        const auto native = engine.price(*option, *context);
        check_price(fixture, native);
        for (const auto& [name, measure] : measures)
            REQUIRE(native->has(measure) == (name == "price"));
        for (const auto& [name, value] : fixture.outputs)
            REQUIRE(measures.contains(name));
        ++generated;
        REQUIRE(fixture.case_id.starts_with("ql-binary-"));
        const bool boundary =
            number("spot") == number("barrier") || (date("expiry") - date("valuation")).count() <= 2;
        REQUIRE(inputs.at("wrapper") == (boundary ? "false" : "true"));
        if (boundary) continue;
        ++wrappers;
        const kiyosi::NumericalShiftSettings shifts{number("spot_shift"), number("volatility_shift"),
                                                    number("rate_shift"), static_cast<int>(number("time_shift_days"))};
        REQUIRE(std::abs(number("spot") - number("barrier")) > 3 * shifts.spot_shift);
        const auto numerical = kiyosi::NumericalAnalyticsEngine{engine, shifts}.price(*option, *context);
        REQUIRE(numerical.has_value());
        for (const auto& [name, measure] : measures) {
            INFO("measure=" << name);
            REQUIRE(numerical->has(measure));
            CHECK_THAT(*numerical->require(measure),
                       Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                                                  number("numerical_tolerance_" + name) +
                                                      number("uncertainty_" + name)));
        }
    }
    CHECK(generated == 268);
    CHECK(wrappers == 140);
}
