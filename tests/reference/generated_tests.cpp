#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <map>
#include <set>
#include <string>
#include <type_traits>

#include "support/reference_harness.hpp"

using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("QuantLib generated references validate all Greeks and boundary declarations")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    std::set<std::string> identifiers;
    std::size_t compared = 0;
    std::set<std::string> compared_engines;
    std::map<std::string, int> wrappers;
    std::map<std::string, int> digital_rows;
    for (const auto& fixture : cases) {
        INFO("case=" << fixture.case_id);
        REQUIRE(identifiers.insert(fixture.case_id).second);
        const auto& inputs = fixture.inputs;
        const bool owned = inputs.contains("owner") && inputs.at("owner") == "QuantLib";
        REQUIRE(owned == fixture.case_id.starts_with("ql-"));
        REQUIRE(owned);
        if (fixture.instrument == "BarrierOption" || fixture.instrument == "BinaryBarrierOption") continue;
        if (fixture.instrument == "GeometricAverageOption" || fixture.instrument == "ArithmeticAverageOption") continue;
        if (fixture.engine == "BinomialAmericanEngine" || fixture.engine == "BinomialEuropeanEngine") continue;
        const bool american = fixture.instrument == "AmericanOption";
        const bool digital = fixture.instrument == "EuropeanCashOrNothingOption" || fixture.instrument == "EuropeanAssetOrNothingOption";
        if (digital) ++digital_rows[fixture.engine];
        REQUIRE((american || digital || fixture.instrument == "EuropeanOption"));
        REQUIRE(fixture.case_id.starts_with(american ? "ql-american-" : digital ? "ql-digital-"
                                                                                : "ql-european-"));

        REQUIRE(fixture.outputs.contains("price"));
        REQUIRE_FALSE(fixture.validation.has_value());
        REQUIRE_FALSE(fixture.convergence.has_value());
        REQUIRE(fixture.monte_carlo.has_value() == (fixture.engine == "MonteCarloEuropeanEngine" || fixture.engine == "MonteCarloAmericanEngine"));
        REQUIRE(fixture.provenance.convention == "Actual/365 Fixed, continuously compounded BSM");
        REQUIRE(fixture.provenance.reference_kind == (american ? "discretized" : "analytic"));
        REQUIRE(fixture.provenance.source_symbol == (american ? "QuantLib.FdBlackScholesVanillaEngine" : "QuantLib.AnalyticEuropeanEngine"));
        REQUIRE(fixture.provenance.explicit_tolerance == fixture.tolerances.at("price"));
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE(fixture.variant == inputs.at("option"));
        REQUIRE(date("effective") <= date("valuation"));
        REQUIRE(date("valuation") < date("expiry"));
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, number("spot"), date("valuation"));
        REQUIRE(context.has_value());
        const auto check_contract = [&](const auto& option) {
            const auto check_engine = [&](const auto& engine) {
                const auto native = engine.price(option, *context);
                check_price(fixture, native);
                const bool expiry_boundary = (date("expiry") - date("valuation")).count() <= 2;
                const bool exercise_boundary = american && (date("valuation") - date("effective")).count() < 2;
                const bool boundary = expiry_boundary || exercise_boundary;
                const bool analytic = fixture.engine == "AnalyticEuropeanEngine";
                if (!analytic) REQUIRE((inputs.at("wrapper") == "true" || inputs.at("wrapper") == "false"));
                const bool wrapped = analytic || inputs.at("wrapper") == "true";
                if (digital && boundary) REQUIRE_FALSE(wrapped);
                if (wrapped) ++wrappers[fixture.engine];
                kiyosi::NumericalShiftSettings shifts;
                if (!analytic) {
                    shifts = {number("spot_shift"), number("volatility_shift"), number("rate_shift"),
                              static_cast<int>(number("time_shift_days"))};
                }
                const auto numerical = wrapped ? kiyosi::NumericalAnalyticsEngine{engine, shifts}.price(option, *context) : native;
                REQUIRE(numerical.has_value());
                std::size_t available = 0;
                for (const auto& [name, measure] : measures) {
                    INFO("measure=" << name);
                    const bool unavailable = boundary && (name == "theta" || name == "charm" || name == "color");
                    REQUIRE(inputs.contains("unit_" + name));
                    REQUIRE(inputs.contains("unavailable_" + name) == unavailable);
                    REQUIRE(fixture.outputs.contains(name) != unavailable);
                    if (unavailable) {
                        REQUIRE(inputs.at("unavailable_" + name) == (expiry_boundary ? "whole-day stability stencil touches expiry" : "whole-day stability stencil precedes exercise window"));
                        continue;
                    }
                    ++available;
                    const bool native_measure = analytic || name == "price" ||
                                                ((fixture.engine == "CrrEngine" ||
                                                  fixture.engine == "FiniteDifferenceEuropeanEngine" ||
                                                  fixture.engine == "FiniteDifferenceAmericanEngine" || fixture.engine == "AnalyticDigitalEngine" ||
                                                  fixture.engine == "FiniteDifferenceDigitalEngine") &&
                                                 (name == "delta" || name == "gamma"));
                    REQUIRE(native->get(measure).has_value() == native_measure);
                    const double expected = fixture.outputs.at(name);
                    if (native_measure)
                        CHECK_THAT(*native->get(measure), Catch::Matchers::WithinAbs(expected, fixture.tolerances.at(name) + number("uncertainty_" + name)));
                    REQUIRE(number("uncertainty_" + name) >= 0);
                    if (wrapped) {
                        REQUIRE(numerical->get(measure).has_value());
                        CHECK_THAT(*numerical->get(measure), Catch::Matchers::WithinAbs(expected, number("numerical_tolerance_" + name) + number("uncertainty_" + name)));
                    }
                }
                REQUIRE(fixture.outputs.size() == available);
            };
            constexpr bool american_contract = std::is_same_v<std::remove_cvref_t<decltype(option)>, kiyosi::AmericanOption>;
            using FiniteDifference = kiyosi::FiniteDifferenceVanillaEngine;
            using MonteCarlo = kiyosi::MonteCarloVanillaEngine;
            constexpr bool digital_contract = std::is_same_v<std::remove_cvref_t<decltype(option)>, kiyosi::EuropeanCashOrNothingOption> ||
                                              std::is_same_v<std::remove_cvref_t<decltype(option)>, kiyosi::EuropeanAssetOrNothingOption>;
            if constexpr (digital_contract) {
                REQUIRE(inputs.at("payoff") == (std::is_same_v<std::remove_cvref_t<decltype(option)>, kiyosi::EuropeanCashOrNothingOption> ? "cash" : "asset"));
                REQUIRE(inputs.at("payoff_condition") == "strict ITM, zero at strike");
                REQUIRE(inputs.at("settlement") == "expiry");
                if (fixture.engine == "AnalyticDigitalEngine") check_engine(kiyosi::AnalyticDigitalEngine{});
                else if (fixture.engine == "IntegralDigitalEngine") check_engine(kiyosi::IntegralDigitalEngine{});
                else if (fixture.engine == "FiniteDifferenceDigitalEngine") {
                    REQUIRE(inputs.at("scheme") == "crank_nicolson");
                    check_engine(kiyosi::FiniteDifferenceDigitalEngine{{static_cast<int>(number("asset_steps")),
                                                                        static_cast<int>(number("time_steps")), kiyosi::finite_difference_scheme::crank_nicolson, number("upper_boundary")}});
                } else FAIL("Unknown digital engine: " << fixture.engine);
            } else {
                if (fixture.engine == "AnalyticEuropeanEngine") {
                    if constexpr (!american_contract) {
                        const kiyosi::AnalyticVanillaEngine engine;
                        check_engine(engine);
                        const auto implied = kiyosi::implied_volatility(
                            engine, option, *context, fixture.outputs.at("price"));
                        INFO("implied volatility: " << (implied ? "ok" : implied.error().message));
                        REQUIRE(implied.has_value());
                        CHECK_THAT(*implied, Catch::Matchers::WithinAbs(number("volatility"), 1e-7));
                    } else {
                        FAIL("AnalyticEuropeanEngine cannot price an American contract");
                    }
                } else if (fixture.engine == "BjerksundStenslandAmericanEngine") {
                    if constexpr (american_contract) check_engine(kiyosi::BjerksundStenslandVanillaEngine{});
                    else FAIL("BjerksundStenslandAmericanEngine requires an American contract");
                } else if (fixture.engine == "CrrEngine") {
                    check_engine(kiyosi::CrrVanillaEngine{static_cast<int>(number("steps"))});
                } else if (fixture.engine == "IntegralEuropeanEngine") {
                    if constexpr (!american_contract) check_engine(kiyosi::IntegralVanillaEngine{});
                    else FAIL("IntegralEuropeanEngine requires a European contract");
                } else if (fixture.engine == (american ? "FiniteDifferenceAmericanEngine" : "FiniteDifferenceEuropeanEngine")) {
                    const std::map<std::string, kiyosi::finite_difference_scheme> schemes{
                        {"explicit_euler", kiyosi::finite_difference_scheme::explicit_euler},
                        {"implicit_euler", kiyosi::finite_difference_scheme::implicit_euler},
                        {"crank_nicolson", kiyosi::finite_difference_scheme::crank_nicolson}};
                    REQUIRE(schemes.contains(inputs.at("scheme")));
                    check_engine(FiniteDifference{{static_cast<int>(number("asset_steps")),
                                                   static_cast<int>(number("time_steps")), schemes.at(inputs.at("scheme")), number("upper_boundary")}});
                } else if (fixture.engine == (american ? "MonteCarloAmericanEngine" : "MonteCarloEuropeanEngine")) {
                    const auto& mc = *fixture.monte_carlo;
                    REQUIRE(mc.tolerance == fixture.tolerances.at("price"));
                    REQUIRE(number("seed") == mc.seed);
                    REQUIRE(number("paths") == mc.paths);
                    REQUIRE(number("steps") == mc.steps);
                    check_engine(MonteCarlo{static_cast<int>(mc.paths), static_cast<int>(mc.steps), mc.seed});
                } else {
                    FAIL("Unknown generated engine: " << fixture.engine);
                }
            }
        };
        const auto type = inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put;
        if (fixture.instrument == "EuropeanCashOrNothingOption") {
            const auto option = kiyosi::make_cash_or_nothing_option(type, number("strike"), number("payout"), date("effective"), date("expiry"));
            REQUIRE(option.has_value());
            check_contract(*option);
        } else if (fixture.instrument == "EuropeanAssetOrNothingOption") {
            const auto option = kiyosi::make_asset_or_nothing_option(type, number("strike"), date("effective"), date("expiry"));
            REQUIRE(option.has_value());
            check_contract(*option);
        } else if (american) {
            const auto option = kiyosi::make_american_option(type, number("strike"), date("effective"), date("expiry"));
            REQUIRE(option.has_value());
            check_contract(*option);
        } else {
            const auto option = kiyosi::make_european_option(type, number("strike"), date("effective"), date("expiry"));
            REQUIRE(option.has_value());
            check_contract(*option);
        }
        compared_engines.insert(fixture.engine);
        ++compared;
    }
    REQUIRE(compared > 0);
    REQUIRE(compared_engines.size() == 11);
    for (const auto& engine : compared_engines)
        REQUIRE(wrappers[engine] >= 2);
    REQUIRE(digital_rows.size() == 3);
    for (const auto& [engine, count] : digital_rows) {
        REQUIRE(count == 40);
        REQUIRE(wrappers[engine] == (engine == "FiniteDifferenceDigitalEngine" ? 4 : 36));
    }
}
