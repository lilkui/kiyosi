#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <string>

#include "support/reference_harness.hpp"

using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("Asian QuantLib references reconstruct averaging contracts and approximate Greeks")
{
    std::size_t generated = 0;
    std::size_t wrapped = 0;
    for (const auto& fixture : kiyosi::test::load_reference_cases(kiyosi::test::fixture_path())) {
        if (fixture.instrument != "GeometricAveragePriceOption" &&
            fixture.instrument != "ArithmeticAveragePriceOption")
            continue;
        const auto& inputs = fixture.inputs;
        INFO("case=" << fixture.case_id);
        REQUIRE(inputs.at("owner") == "QuantLib");
        REQUIRE(fixture.case_id.starts_with("ql-asian-"));
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        const bool geometric = fixture.instrument == "GeometricAveragePriceOption";
        REQUIRE(inputs.at("averaging") == (geometric ? "geometric" : "arithmetic"));
        REQUIRE(inputs.at("monitoring") == "continuous");
        REQUIRE(inputs.at("settlement") == "expiry_date");
        REQUIRE(inputs.at("date_roll") == "none");
        REQUIRE(fixture.provenance.convention == "Actual/365 Fixed, continuously compounded BSM");
        REQUIRE((inputs.at("calendar") == "null" || inputs.at("calendar") == "sse"));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        const auto type = inputs.at("option") == "call" ? kiyosi::OptionType::call : kiyosi::OptionType::put;
        REQUIRE(date("effective_date") <= date("valuation"));
        REQUIRE(date("averaging_start_date") <= date("valuation"));
        REQUIRE(date("valuation") <= date("expiry_date"));
        const bool terminal = date("valuation") == date("expiry_date");
        REQUIRE(fixture.provenance.source_symbol ==
                (terminal    ? "QuantLib.PlainVanillaPayoff"
                 : geometric ? "QuantLib.AnalyticContinuousGeometricAveragePriceAsianEngine"
                             : "QuantLib.ContinuousArithmeticAsianLevyEngine"));
        REQUIRE(fixture.provenance.reference_kind == (!geometric && !terminal ? "approximate" : "analytic"));
        if (geometric) {
            REQUIRE(date("averaging_start_date") == date("valuation"));
            REQUIRE(number("realized_average") == 0);
        }
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(
            *parameters, number("spot"), date("valuation"),
            inputs.at("calendar") == "sse" ? kiyosi::sse_calendar() : kiyosi::all_days_calendar());
        REQUIRE(context.has_value());
        const auto check = [&](const auto& option, const auto& engine) {
            REQUIRE(option.has_value());
            const auto native = engine.price(*option, *context);
            check_price(fixture, native);
            ++generated;
            const bool smooth = !terminal && (date("valuation") - date("averaging_start_date")).count() > 2 &&
                                (date("expiry_date") - date("valuation")).count() > 2;
            REQUIRE(inputs.at("wrapper") == (smooth ? "true" : "false"));
            if (!smooth) {
                REQUIRE(fixture.outputs.size() == 1);
                return;
            }
            ++wrapped;
            REQUIRE(fixture.outputs.size() == measures.size());
            const auto numerical = kiyosi::calculate_numerical_risk_measures(
                engine, *option, *context,
                kiyosi::NumericalShiftSettings{number("spot_shift"), number("volatility_shift"),
                                               number("rate_shift"),
                                               static_cast<int>(number("time_shift_days"))});
            REQUIRE(numerical.has_value());
            for (const auto& [name, measure] : measures) {
                INFO("measure=" << name);
                REQUIRE(numerical->has(measure));
                CHECK_THAT(*numerical->require(measure),
                           Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                                                      number("numerical_tolerance_" + name) +
                                                          number("uncertainty_" + name)));
            }
        };
        if (geometric) {
            REQUIRE(fixture.engine == "AnalyticGeometricAveragePriceEngine");
            check(kiyosi::make_geometric_average_option(type, number("strike"), date("averaging_start_date"),
                                                        date("effective_date"), date("expiry_date"),
                                                        number("realized_average")),
                  kiyosi::AnalyticGeometricAveragePriceEngine{});
        } else {
            REQUIRE(fixture.engine == "TurnbullWakemanArithmeticAveragePriceEngine");
            check(kiyosi::make_arithmetic_average_option(type, number("strike"), date("averaging_start_date"),
                                                         date("effective_date"), date("expiry_date"),
                                                         number("realized_average")),
                  kiyosi::TurnbullWakemanArithmeticAveragePriceEngine{});
        }
    }
    CHECK(generated == 24);
    CHECK(wrapped == 6);
}

TEST_CASE("Geometric Asian pricing uses the realized and remaining averaging periods")
{
    const auto effective = kiyosi::Date{std::chrono::year{2025} / 1 / 1};
    const auto valuation = kiyosi::Date{std::chrono::year{2025} / 7 / 1};
    const auto expiry = kiyosi::Date{std::chrono::year{2026} / 1 / 1};
    const auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(parameters);
    const auto context = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
    REQUIRE(context);
    const kiyosi::AnalyticGeometricAveragePriceEngine engine;

    struct Case { kiyosi::Date start; double realized; double expected; };
    const std::array cases{
        Case{effective, 80.0, 0.005141652127146822},
        Case{effective, 120.0, 9.472410353379193},
        Case{kiyosi::Date{std::chrono::year{2025} / 10 / 1}, 0.0, 5.064920706779442},
    };
    for (const auto& test : cases) {
        const auto option = kiyosi::make_geometric_average_option(
            kiyosi::OptionType::call, 100.0, test.start, effective, expiry, test.realized);
        REQUIRE(option);
        const auto price = engine.price(*option, *context);
        REQUIRE(price);
        CHECK_THAT(*price, Catch::Matchers::WithinAbs(test.expected, 1e-10));
    }

    const auto missing = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, effective, effective, expiry);
    REQUIRE(missing);
    const auto missing_price = engine.price(*missing, *context);
    REQUIRE_FALSE(missing_price);
    CHECK(missing_price.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto starting = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, valuation, effective, expiry);
    REQUIRE(starting);
    const auto at_start = engine.price_with_greeks(*starting, *context, kiyosi::GreeksLevel::full);
    REQUIRE(at_start);
    CHECK(at_start->has(kiyosi::RiskMeasure::delta));
    CHECK_FALSE(at_start->has(kiyosi::RiskMeasure::theta));

    const auto ending = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, effective, effective, expiry, 120.0);
    REQUIRE(ending);
    const auto near_expiry = kiyosi::make_pricing_context(
        *parameters, 100.0, kiyosi::Date{std::chrono::year{2025} / 12 / 31});
    const auto at_expiry = kiyosi::make_pricing_context(*parameters, 100.0, expiry);
    REQUIRE(near_expiry);
    REQUIRE(at_expiry);
    const auto near_price = engine.price(*ending, *near_expiry);
    REQUIRE(near_price);
    CHECK_THAT(*near_price, Catch::Matchers::WithinAbs(19.937346821828626, 1e-10));
    const auto expiry_price = engine.price(*ending, *at_expiry);
    REQUIRE(expiry_price);
    CHECK(*expiry_price == 20.0);
}
