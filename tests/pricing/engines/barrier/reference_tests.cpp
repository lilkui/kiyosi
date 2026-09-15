#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <map>
#include <string>

#include "support/reference_harness.hpp"

using kiyosi::test::barrier_kinds;
using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("QuantLib continuous barrier portfolios validate prices and numerical Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    std::map<std::string, int> generated, wrappers;
    for (const auto& fixture : cases) {
        const auto& inputs = fixture.inputs;
        if (fixture.instrument != "BarrierOption") continue;
        INFO("case=" << fixture.case_id);
        REQUIRE(inputs.at("owner") == "QuantLib");
        REQUIRE(fixture.provenance.reference_kind == "analytic");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBarrierEngine+QuantLib.DiscountingBondEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        REQUIRE(barrier_kinds.contains(inputs.at("barrier_kind")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE((inputs.at("settlement") == "at_hit" || inputs.at("settlement") == "at_expiry"));
        const auto option = kiyosi::make_barrier_option({
            .type = inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            .strike = number("strike"),
            .effective = date("effective"),
            .expiry = date("expiry"),
            .barrier = number("barrier"),
            .barrier_kind = barrier_kinds.at(inputs.at("barrier_kind")),
            .rebate = number("rebate"),
            .rebate_payment = inputs.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit
                                                                   : kiyosi::rebate_timing::at_expiry});
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, number("spot"), date("valuation"));
        REQUIRE(context.has_value());
        const auto check = [&](const auto& engine) {
            const auto native = engine.price(*option, *context);
            check_price(fixture, native);
            for (const auto& [name, measure] : measures)
                REQUIRE(native->get(measure).has_value() == (name == "price"));
            ++generated[fixture.engine];
            REQUIRE((inputs.at("wrapper") == "true" || inputs.at("wrapper") == "false"));
            const bool boundary = (date("expiry") - date("valuation")).count() <= 2;
            if (boundary) REQUIRE(inputs.at("wrapper") == "false");
            for (const auto& [name, value] : fixture.outputs)
                REQUIRE(measures.contains(name));
            if (inputs.at("wrapper") == "false") return;
            ++wrappers[fixture.engine];
            const kiyosi::NumericalShiftSettings shifts{number("spot_shift"), number("volatility_shift"),
                                                        number("rate_shift"), static_cast<int>(number("time_shift_days"))};
            // Three nested spot shifts are used by speed; keep every stencil in the same hit state.
            REQUIRE(std::abs(number("spot") - number("barrier")) > 3 * shifts.spot_shift);
            const auto numerical = kiyosi::NumericalAnalyticsEngine{engine, shifts}.price(*option, *context);
            REQUIRE(numerical.has_value());
            for (const auto& [name, measure] : measures) {
                INFO("measure=" << name);
                REQUIRE(fixture.outputs.contains(name));
                REQUIRE(numerical->get(measure).has_value());
                CHECK_THAT(*numerical->get(measure),
                           Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                                                      number("numerical_tolerance_" + name) +
                                                          number("uncertainty_" + name)));
            }
        };
        if (fixture.engine == "AnalyticBarrierEngine") check(kiyosi::AnalyticBarrierEngine{});
        else {
            REQUIRE(fixture.engine == "FiniteDifferenceBarrierEngine");
            REQUIRE(inputs.at("scheme") == "crank_nicolson");
            check(kiyosi::FiniteDifferenceBarrierEngine{
                {static_cast<int>(number("asset_steps")), static_cast<int>(number("time_steps")),
                 kiyosi::finite_difference_scheme::crank_nicolson, number("upper_boundary")}});
        }
    }
    REQUIRE(generated.size() == 2);
    CHECK(generated["AnalyticBarrierEngine"] == 60);
    CHECK(generated["FiniteDifferenceBarrierEngine"] == 60);
    CHECK(wrappers["AnalyticBarrierEngine"] == 48);
    CHECK(wrappers["FiniteDifferenceBarrierEngine"] == 12);
}
