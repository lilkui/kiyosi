#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>

#include <array>
#include <filesystem>
#include <fstream>
#include <cmath>
#include <chrono>
#include <limits>
#include <ranges>
#include <set>
#include <sstream>
#include <string>
#include <type_traits>

#include "support/reference_fixture.hpp"

namespace {

const std::map<std::string, kiyosi::risk_measure> measures{
        {"price", kiyosi::risk_measure::price}, {"delta", kiyosi::risk_measure::delta},
        {"gamma", kiyosi::risk_measure::gamma}, {"speed", kiyosi::risk_measure::speed},
        {"theta", kiyosi::risk_measure::theta}, {"charm", kiyosi::risk_measure::charm},
        {"color", kiyosi::risk_measure::color}, {"vega", kiyosi::risk_measure::vega},
        {"vanna", kiyosi::risk_measure::vanna}, {"zomma", kiyosi::risk_measure::zomma},
        {"rho", kiyosi::risk_measure::rho}};

std::filesystem::path fixture_path()
{
    return std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests" / "fixtures" / "pricing_reference.tsv";
}

std::string fixture_text()
{
    std::ifstream input(fixture_path());
    REQUIRE(input.good());
    return {std::istreambuf_iterator<char>{input}, std::istreambuf_iterator<char>{}};
}

kiyosi::PricingContext standard_context()
{
    const auto valuation = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    return *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), *kiyosi::make_asset_price(100.0), valuation);
}

kiyosi::date standard_expiry()
{
    return kiyosi::date{std::chrono::year{2026} / 1 / 6};
}

template <typename PriceResult>
void check_price(const kiyosi::test::ReferenceCase& fixture, const PriceResult& priced)
{
    INFO("case=" << fixture.case_id << " instrument=" << fixture.instrument << " engine=" << fixture.engine);
    REQUIRE(priced.has_value());
    REQUIRE(fixture.outputs.contains("price"));
    REQUIRE(fixture.tolerances.contains("price"));
    REQUIRE(priced->get(kiyosi::risk_measure::price).has_value());
    CAPTURE(*priced->get(kiyosi::risk_measure::price));
    CHECK(std::abs(*priced->get(kiyosi::risk_measure::price) - fixture.outputs.at("price")) <=
          fixture.tolerances.at("price"));
}

} // namespace

TEST_CASE("QuantLib generated references validate all Greeks and boundary declarations")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    std::set<std::string> identifiers;
    std::size_t compared = 0;
    std::set<std::string> compared_engines;
    std::size_t migrated = 0;
    std::size_t converged = 0;
    std::map<std::string, int> wrappers;
    std::map<std::string, int> digital_rows;
    for (const auto& fixture : cases) {
        INFO("case=" << fixture.case_id);
        REQUIRE(identifiers.insert(fixture.case_id).second);
        const auto& inputs = fixture.inputs;
        const bool owned = inputs.contains("owner") && inputs.at("owner") == "QuantLib";
        REQUIRE(owned == fixture.case_id.starts_with("ql-"));
        const bool legacy = inputs.contains("reference_provider") && inputs.at("reference_provider") == "QuantLib";
        if (!owned && !legacy) continue;
        if (fixture.instrument == "BarrierOption" || fixture.instrument == "BinaryBarrierOption") continue;
        if (fixture.instrument == "GeometricAverageOption" || fixture.instrument == "ArithmeticAverageOption") continue;
        const bool american = fixture.instrument == "AmericanOption";
        const bool digital = fixture.instrument == "EuropeanCashOrNothingOption" || fixture.instrument == "EuropeanAssetOrNothingOption";
        if (digital && owned) ++digital_rows[fixture.engine];
        REQUIRE((american || digital || fixture.instrument == "EuropeanOption"));
        if (owned) REQUIRE(fixture.case_id.starts_with(american ? "ql-american-" : digital ? "ql-digital-" : "ql-european-"));

        REQUIRE(fixture.outputs.contains("price"));
        REQUIRE_FALSE(fixture.validation.has_value());
        if (owned) REQUIRE_FALSE(fixture.convergence.has_value());
        REQUIRE(fixture.monte_carlo.has_value() == (fixture.engine == "MonteCarloEuropeanEngine" || fixture.engine == "MonteCarloAmericanEngine"));
        REQUIRE(fixture.provenance.convention == "Actual/365 Fixed, continuously compounded BSM");
        REQUIRE(fixture.provenance.reference_kind == (american ? "discretized" : "analytic"));
        REQUIRE(fixture.provenance.source_symbol == (american ? "QuantLib.FdBlackScholesVanillaEngine" : "QuantLib.AnalyticEuropeanEngine"));
        REQUIRE(fixture.provenance.explicit_tolerance == fixture.tolerances.at("price"));
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE((fixture.variant == inputs.at("option") || (legacy && (fixture.variant == "seeded-call" || fixture.variant == "seeded-put"))));
        REQUIRE(date("effective") <= date("valuation"));
        REQUIRE(date("valuation") < date("expiry"));
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"));
        REQUIRE(context.has_value());
        const auto check_contract = [&](const auto& option) {
            const auto check_engine = [&](const auto& engine) {
                const auto native = engine.price(option, *context);
                check_price(fixture, native);
                if (legacy) {
                    REQUIRE(fixture.outputs.size() == 1);
                    if (fixture.convergence) {
                        const auto& convergence = *fixture.convergence;
                        REQUIRE(convergence.reference == fixture.outputs.at("price"));
                        double first_error = 0;
                        double last_error = 0;
                        for (const auto resolution : convergence.resolutions) {
                            auto refined = engine;
                            if constexpr (requires { engine.settings().asset_steps; }) {
                                REQUIRE(convergence.parameter == "grid");
                                auto settings = engine.settings();
                                settings.asset_steps = static_cast<int>(resolution);
                                refined = std::remove_cvref_t<decltype(engine)>{settings};
                            } else if constexpr (requires { engine.settings().steps; }) {
                                REQUIRE(convergence.parameter == "steps");
                                refined = std::remove_cvref_t<decltype(engine)>{static_cast<int>(resolution)};
                            } else {
                                FAIL("Unexpected convergence engine");
                            }
                            const auto result = refined.price(option, *context);
                            REQUIRE(result.has_value());
                            REQUIRE(result->get(kiyosi::risk_measure::price).has_value());
                            last_error = std::abs(*result->get(kiyosi::risk_measure::price) - convergence.reference);
                            if (resolution == convergence.resolutions.front()) first_error = last_error;
                        }
                        CHECK(last_error < first_error);
                        CHECK(last_error <= convergence.tolerance);
                        ++converged;
                    }
                    ++migrated;
                    return;
                }
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
                        REQUIRE(inputs.at("unavailable_" + name) == (expiry_boundary ? "whole-day stability stencil touches expiry" :
                            "whole-day stability stencil precedes exercise window"));
                        continue;
                    }
                    ++available;
                    const bool native_measure = analytic || name == "price" ||
                        ((fixture.engine == "BinomialEuropeanEngine" || fixture.engine == "CrrEngine" ||
                          fixture.engine == "FiniteDifferenceEuropeanEngine" || fixture.engine == "BinomialAmericanEngine" ||
                          fixture.engine == "FiniteDifferenceAmericanEngine" || fixture.engine == "AnalyticDigitalEngine" ||
                          fixture.engine == "FiniteDifferenceDigitalEngine") && (name == "delta" || name == "gamma"));
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
            using Binomial = std::conditional_t<american_contract, kiyosi::BinomialAmericanEngine, kiyosi::BinomialEuropeanEngine>;
            using FiniteDifference = std::conditional_t<american_contract, kiyosi::FiniteDifferenceAmericanEngine, kiyosi::FiniteDifferenceEuropeanEngine>;
            using MonteCarlo = std::conditional_t<american_contract, kiyosi::MonteCarloAmericanEngine, kiyosi::MonteCarloEuropeanEngine>;
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
                        const kiyosi::AnalyticEuropeanEngine engine;
                        check_engine(engine);
                        const auto implied = engine.implied_volatility(option, *context, fixture.outputs.at("price"));
                        INFO("implied volatility: " << (implied ? "ok" : implied.error().message));
                        REQUIRE(implied.has_value());
                        CHECK_THAT(*implied, Catch::Matchers::WithinAbs(number("volatility"), 1e-7));
                    } else {
                        FAIL("AnalyticEuropeanEngine cannot price an American contract");
                    }
                } else if (fixture.engine == "BjerksundStenslandAmericanEngine") {
                    if constexpr (american_contract) check_engine(kiyosi::BjerksundStenslandAmericanEngine{});
                    else FAIL("BjerksundStenslandAmericanEngine requires an American contract");
                } else if (fixture.engine == (american ? "BinomialAmericanEngine" : "BinomialEuropeanEngine")) {
                    check_engine(Binomial{static_cast<int>(number("steps"))});
                } else if (fixture.engine == "CrrEngine") {
                    check_engine(kiyosi::CrrEngine{static_cast<int>(number("steps"))});
                } else if (fixture.engine == "IntegralEuropeanEngine") {
                    if constexpr (!american_contract) check_engine(kiyosi::IntegralEuropeanEngine{});
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
    REQUIRE(compared_engines.size() == 13);
    REQUIRE(migrated == 13);
    REQUIRE(converged == 6);
    for (const auto& engine : compared_engines) REQUIRE(wrappers[engine] >= 2);
    REQUIRE(digital_rows.size() == 3);
    for (const auto& [engine, count] : digital_rows) {
        REQUIRE(count == 40);
        REQUIRE(wrappers[engine] == (engine == "FiniteDifferenceDigitalEngine" ? 4 : 36));
    }
}

TEST_CASE("QuantLib fixture parser rejects missing or invalid Greek declarations")
{
    const auto original = fixture_text();
    for (const auto identifier : {"ql-european-call-100-1d\t",
             "ql-digital-cash-call-100-1d-analyticdigitalengine\t",
             "ql-digital-asset-put-100-1d-analyticdigitalengine\t",
             "ql-barrier-call-up-and-out-at-expiry-110-1d-analyticbarrierengine\t",
             "ql-binary-cash-call-up-and-in-at-expiry-110-100-1d\t"}) {
        const auto start = original.find(identifier);
        REQUIRE(start != std::string::npos);
        const auto end = original.find('\n', start);
        const auto row = original.substr(start, end - start);
        for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
                 {"unavailable_theta=whole-day stability stencil touches expiry;", ""},
                 {"unavailable_theta=whole-day stability stencil touches expiry", "unavailable_theta=unknown"},
                 {"unit_vega=price/volatility-pp", "unit_vega=price/volatility"},
                 {"uncertainty_price=0", "uncertainty_price=nan"},
                 {"uncertainty_price=0", "uncertainty_price=10"},
                 {"unit_price=price", "unit_price=price;unavailable_typo=unknown"}}) {
            auto changed = row;
            const auto position = changed.find(from);
            REQUIRE(position != std::string::npos);
            changed.replace(position, from.size(), to);
            auto text = original;
            text.replace(start, row.size(), changed);
            std::istringstream input{text};
            CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(input), kiyosi::test::FixtureParseError);
        }
    }
}

TEST_CASE("Digital expiry settlement uses strict strikes without smooth Greeks")
{
    struct Settlement { kiyosi::option_type type; double spot; double cash; double asset; };
    // These exact settlement values are also checked against QuantLib payoff bindings.
    const std::array cases{
        Settlement{kiyosi::option_type::call, 99, 0, 0}, Settlement{kiyosi::option_type::call, 100, 0, 0},
        Settlement{kiyosi::option_type::call, 101, 10, 101}, Settlement{kiyosi::option_type::put, 99, 10, 99},
        Settlement{kiyosi::option_type::put, 100, 0, 0}, Settlement{kiyosi::option_type::put, 101, 0, 0}};
    const auto expiry = standard_expiry();
    for (const auto& item : cases) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), *kiyosi::make_asset_price(item.spot), expiry);
        const auto cash = *kiyosi::make_cash_or_nothing_option(item.type, 100, 10, expiry);
        const auto asset = *kiyosi::make_asset_or_nothing_option(item.type, 100, expiry);
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

TEST_CASE("American reference fixtures reject unknown measures and exercise boundary omissions")
{
    const auto original = fixture_text();
    const auto start = original.find("ql-american-call-exercise-start-binomialamericanengine\t");
    REQUIRE(start != std::string::npos);
    const auto end = original.find('\n', start);
    const auto row = original.substr(start, end - start);
    for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
             {"unavailable_theta=whole-day stability stencil precedes exercise window;", ""},
             {"unavailable_theta=whole-day stability stencil precedes exercise window", "unavailable_theta=unknown"},
             {"unit_price=price", "unit_price=price;unavailable_typo=unknown"},
             {"\tdelta=", "\ttypo="}}) {
        auto changed = row;
        const auto position = changed.find(from);
        REQUIRE(position != std::string::npos);
        changed.replace(position, from.size(), to);
        auto text = original;
        text.replace(start, row.size(), changed);
        std::istringstream input{text};
        CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(input), kiyosi::test::FixtureParseError);
    }
}

TEST_CASE("Reference fixture parser reports malformed rows")
{
    auto text = fixture_text();
    const auto output = text.find("\tprice=13.151137");
    REQUIRE(output != std::string::npos);
    text.replace(output + 1, std::string{"price=13.151137"}.size(), "price=oops");
    auto parse = [&] {
        std::istringstream input{text};
        return kiyosi::test::parse_reference_cases(input);
    };
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("fixture row 5"));
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("invalid outputs"));

    text = fixture_text();
    const auto tolerance = text.find("price=0.00001");
    REQUIRE(tolerance != std::string::npos);
    text.replace(tolerance, std::string{"price=0.00001"}.size(), "price=-1");
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("non-negative"));

    std::size_t index = 0;
    CHECK_THROWS_WITH(kiyosi::test::detail::calendar_date({"2025-0x-06"}, index, 0, "valuation"),
                      Catch::Matchers::ContainsSubstring("valuation"));
}

TEST_CASE("Reference fixture tolerances are inclusive and mismatch reports are useful")
{
    CHECK(kiyosi::test::within_tolerance(2.0, 1.0, 1.0));
    CHECK_FALSE(kiyosi::test::within_tolerance(2.000001, 1.0, 1.0));

    auto fixture = kiyosi::test::load_reference_cases(fixture_path()).front();
    const auto actual = fixture.outputs;
    CHECK(kiyosi::test::compare_fixture(fixture, actual).empty());
    fixture.outputs.at("price") += 1.0;
    const auto failures = kiyosi::test::compare_fixture(fixture, actual);
    REQUIRE(failures.size() == 1);
    const auto message = failures.front().message();
    CHECK(message.find("case='european-analytic'") != std::string::npos);
    CHECK(message.find("output='price'") != std::string::npos);
    CHECK(message.find("expected=") != std::string::npos);
    CHECK(message.find("actual=") != std::string::npos);
    CHECK(message.find("tolerance=") != std::string::npos);
}

TEST_CASE("Pricing reference manifest covers instruments, engines, and numerical metadata")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    REQUIRE(cases.size() >= 20);
    std::vector<std::string> names;
    names.reserve(cases.size());
    for (const auto& value : cases) {
        INFO("case=" << value.case_id << " instrument=" << value.instrument << " engine=" << value.engine);
        REQUIRE_FALSE(value.case_id.empty());
        REQUIRE_FALSE(value.instrument.empty());
        REQUIRE_FALSE(value.engine.empty());
        REQUIRE_FALSE(value.variant.empty());
        CHECK_FALSE(value.provenance.source_symbol.empty());
        CHECK_FALSE(value.provenance.convention.empty());
        CHECK((value.provenance.reference_kind == "analytic" ||
               value.provenance.reference_kind == "approximate" ||
               value.provenance.reference_kind == "discretized" ||
               value.provenance.reference_kind == "statistical"));
        REQUIRE(value.outputs.size() == value.tolerances.size());
        for (const auto& [name, tolerance] : value.tolerances) {
            REQUIRE(value.outputs.contains(name));
            CHECK(tolerance >= 0.0);
        names.push_back(value.instrument + "/" + value.engine);
        }
    }
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "Accumulator/MonteCarloAccumulatorEngine"; }));
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "PhoenixOption/FiniteDifferencePhoenixEngine"; }));
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "BarrierOption/FiniteDifferenceBarrierEngine"; }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.validation.has_value(); }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.convergence.has_value(); }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.monte_carlo.has_value(); }));
}

TEST_CASE("Pricing reference manifest inventories engines and retained contract variants")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const std::set<std::string> required_engines{
        "AnalyticBarrierEngine", "AnalyticBinaryBarrierEngine", "AnalyticDigitalEngine",
        "AnalyticEuropeanEngine", "ArithmeticAverageAsianEngine", "BinomialAmericanEngine",
        "BinomialEuropeanEngine", "BjerksundStenslandAmericanEngine", "CrrEngine",
        "FiniteDifferenceAccumulatorEngine", "FiniteDifferenceAmericanEngine",
        "FiniteDifferenceBarrierEngine", "FiniteDifferenceBinarySnowballEngine",
        "FiniteDifferenceDigitalEngine", "FiniteDifferenceEuropeanEngine",
        "FiniteDifferencePhoenixEngine", "FiniteDifferenceSnowballEngine",
        "FiniteDifferenceTernarySnowballEngine", "GeometricAverageAsianEngine",
        "IntegralDigitalEngine", "IntegralEuropeanEngine", "MonteCarloAccumulatorEngine",
        "MonteCarloAmericanEngine", "MonteCarloBinarySnowballEngine",
        "MonteCarloEuropeanEngine", "MonteCarloPhoenixEngine", "MonteCarloSnowballEngine",
        "MonteCarloTernarySnowballEngine"};
    const std::set<std::string> required_instruments{
        "Accumulator", "AmericanOption", "ArithmeticAverageOption", "BarrierOption",
        "BermudanOption", "BinaryBarrierOption", "BinarySnowballOption", "EuropeanAssetOrNothingOption",
        "EuropeanCashOrNothingOption", "EuropeanOption", "GeometricAverageOption",
        "PhoenixOption", "SnowballOption", "TernarySnowballOption"};
    const std::set<std::string> required_pairs{
        "Accumulator/FiniteDifferenceAccumulatorEngine", "Accumulator/MonteCarloAccumulatorEngine",
        "AmericanOption/BinomialAmericanEngine", "AmericanOption/FiniteDifferenceAmericanEngine",
        "AmericanOption/MonteCarloAmericanEngine", "AmericanOption/BjerksundStenslandAmericanEngine",
        "AmericanOption/CrrEngine",
        "ArithmeticAverageOption/ArithmeticAverageAsianEngine", "BarrierOption/AnalyticBarrierEngine",
        "BarrierOption/FiniteDifferenceBarrierEngine",
        "BinaryBarrierOption/AnalyticBinaryBarrierEngine", "BinarySnowballOption/FiniteDifferenceBinarySnowballEngine",
        "BinarySnowballOption/MonteCarloBinarySnowballEngine", "EuropeanAssetOrNothingOption/AnalyticDigitalEngine",
        "EuropeanAssetOrNothingOption/IntegralDigitalEngine", "EuropeanAssetOrNothingOption/FiniteDifferenceDigitalEngine",
        "EuropeanCashOrNothingOption/AnalyticDigitalEngine", "EuropeanCashOrNothingOption/FiniteDifferenceDigitalEngine",
        "EuropeanCashOrNothingOption/IntegralDigitalEngine", "EuropeanOption/AnalyticEuropeanEngine",
        "EuropeanOption/BinomialEuropeanEngine", "EuropeanOption/CrrEngine",
        "EuropeanOption/FiniteDifferenceEuropeanEngine", "EuropeanOption/IntegralEuropeanEngine",
        "EuropeanOption/MonteCarloEuropeanEngine", "GeometricAverageOption/GeometricAverageAsianEngine",
        "PhoenixOption/FiniteDifferencePhoenixEngine", "PhoenixOption/MonteCarloPhoenixEngine",
        "SnowballOption/FiniteDifferenceSnowballEngine", "SnowballOption/MonteCarloSnowballEngine",
        "TernarySnowballOption/FiniteDifferenceTernarySnowballEngine",
        "TernarySnowballOption/MonteCarloTernarySnowballEngine"};
    std::set<std::string> actual_engines;
    std::set<std::string> actual_instruments;
    std::set<std::string> actual_pairs;
    bool has_settlement = false;
    bool has_monitoring = false;
    bool has_calendar = false;
    for (const auto& value : cases) {
        actual_engines.insert(value.engine);
        actual_instruments.insert(value.instrument);
        // The retained Bermudan-labelled row is historical data, not a pricing comparison.
        if (value.instrument != "BermudanOption") actual_pairs.insert(value.instrument + "/" + value.engine);
        has_settlement |= value.inputs.contains("settlement");
        has_monitoring |= value.inputs.contains("monitoring");
        has_calendar |= value.inputs.contains("calendar");
        if (value.engine.find("MonteCarlo") != std::string::npos && !value.validation.has_value())
            REQUIRE(value.monte_carlo.has_value());
        if (value.engine.find("Binomial") != std::string::npos ||
            value.engine.find("FiniteDifference") != std::string::npos)
            if (!value.validation.has_value() && value.instrument != "BermudanOption" && !value.case_id.starts_with("ql-"))
            REQUIRE(value.convergence.has_value());
        if (value.validation.has_value())
            CHECK(value.outputs.empty());
        else
            REQUIRE(value.outputs.contains("price"));
    }
    CHECK(actual_engines == required_engines);
    CHECK(actual_instruments == required_instruments);
    CHECK(actual_pairs == required_pairs);
    CHECK(has_settlement);
    CHECK(has_monitoring);
    CHECK(has_calendar);
}

TEST_CASE("European reference fixtures compare price, every Greek, and implied volatility")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());

    for (const auto* id : {"european-analytic", "european-analytic-put"}) {
        const auto fixture = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(fixture != cases.end());
        INFO("case=" << fixture->case_id);
        const auto& inputs = fixture->inputs;
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        const auto option = kiyosi::make_european_option(
            inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            number("strike"), date("expiry"));
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"));
        REQUIRE(context.has_value());
        const kiyosi::AnalyticEuropeanEngine engine;
        const auto result = engine.price(*option, *context);
        REQUIRE(result.has_value());
        const auto implied = engine.implied_volatility(*option, *context, number("observed_price"));
        REQUIRE(implied.has_value());
        std::map<std::string, double> actual{{"implied_volatility", *implied}};
        for (const auto& [name, measure] : measures) {
            REQUIRE(result->get(measure).has_value());
            actual.emplace(name, *result->get(measure));
        }
        REQUIRE(fixture->outputs.size() == actual.size());
        for (const auto& [name, value] : actual) {
            REQUIRE(fixture->outputs.contains(name));
        }
        kiyosi::test::check_fixture(*fixture, actual);
    }
}

TEST_CASE("Vanilla and digital engines match reviewed pricing fixtures")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto context = standard_context();
    const auto expiry = standard_expiry();
    const auto call = *kiyosi::make_european_call(100.0, context.valuation_date(), expiry);
    const auto put = *kiyosi::make_european_put(100.0, context.valuation_date(), expiry);
    const auto find = [&](std::string_view case_id) -> const kiyosi::test::ReferenceCase& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        return *found;
    };

    check_price(find("european-analytic"), kiyosi::AnalyticEuropeanEngine{}.price(call, context));
    check_price(find("european-binomial"), kiyosi::BinomialEuropeanEngine{200}.price(call, context));
    check_price(find("crr-vanilla"), kiyosi::CrrEngine{200}.price(call, context));
    check_price(find("european-integral"), kiyosi::IntegralEuropeanEngine{}.price(put, context));
    check_price(find("european-fd"), kiyosi::FiniteDifferenceEuropeanEngine{
        {200, 4000, kiyosi::finite_difference_scheme::explicit_euler}}.price(call, context));
    const auto cash_call = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 10.0, expiry);
    const auto asset_put = *kiyosi::make_asset_or_nothing_option(kiyosi::option_type::put, 100.0, expiry);
    check_price(find("cash-digital-analytic"), kiyosi::AnalyticDigitalEngine{}.price(cash_call, context));
    check_price(find("asset-digital-analytic"), kiyosi::AnalyticDigitalEngine{}.price(asset_put, context));
    check_price(find("digital-integral"), kiyosi::IntegralDigitalEngine{}.price(cash_call, context));
    const auto digital_put = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::put, 100.0, 10.0, expiry);
    check_price(find("digital-fd"), kiyosi::FiniteDifferenceDigitalEngine{200, 200}.price(digital_put, context));
}

TEST_CASE("Barrier fixtures reconstruct every pinned public variant")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto valuation = standard_context().valuation_date();
    const auto expiry = standard_expiry();
    const auto context = standard_context();
    const auto find = [&](std::string_view id) -> const auto& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(found != cases.end());
        return *found;
    };
    const auto barrier_kind = [](std::string_view value) {
        if (value == "up_and_in") return kiyosi::barrier_type::up_and_in;
        if (value == "up_and_out") return kiyosi::barrier_type::up_and_out;
        if (value == "down_and_in") return kiyosi::barrier_type::down_and_in;
        return kiyosi::barrier_type::down_and_out;
    };
    const auto observations = [&](const auto& input) {
        std::vector<kiyosi::date> dates;
        if (input.contains("interval_days"))
            for (auto date = valuation + std::chrono::days{std::stoi(input.at("interval_days"))};
                 date <= expiry; date += std::chrono::days{std::stoi(input.at("interval_days"))})
                dates.push_back(date);
        return dates;
    };
    const auto vanilla = [&](std::string_view id) {
        const auto& fixture = find(id);
        const auto& input = fixture.inputs;
        const auto option = *kiyosi::make_barrier_option(
            input.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            std::stod(input.at("strike")), valuation, expiry, std::stod(input.at("barrier")),
            barrier_kind(input.at("barrier_kind")), std::stod(input.at("rebate")),
            input.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit : kiyosi::rebate_timing::at_expiry,
            input.contains("monitoring") && input.at("monitoring") == "scheduled"
                ? kiyosi::observation_mode::scheduled : kiyosi::observation_mode::continuous,
            observations(input));
        check_price(fixture, kiyosi::AnalyticBarrierEngine{}.price(option, context));
    };
    for (const auto id : {"barrier-down-in-call", "barrier-down-in-put", "barrier-up-in-call",
                          "barrier-up-in-put", "barrier-down-out-call", "barrier-down-out-put",
                          "barrier-up-out-call", "barrier-up-out-put", "barrier-scheduled-monitoring"})
        vanilla(id);
    for (const auto id : {"barrier-fd-down-in-call", "barrier-fd-down-in-put", "barrier-fd-up-in-call",
                          "barrier-fd-up-in-put", "barrier-fd-down-out-call", "barrier-fd-down-out-put",
                          "barrier-fd-up-out-call", "barrier-fd-up-out-put"}) {
        const auto& fixture = find(id);
        const auto& input = fixture.inputs;
        const auto option = *kiyosi::make_barrier_option(
            input.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            std::stod(input.at("strike")), valuation, expiry, std::stod(input.at("barrier")),
            barrier_kind(input.at("barrier_kind")), std::stod(input.at("rebate")));
        check_price(fixture, kiyosi::FiniteDifferenceBarrierEngine{1000, 1000}.price(option, context));
    }


}

TEST_CASE("Barrier validation fixtures exercise public constructors")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto expiry = standard_expiry();
    const auto expected = [&](std::string_view id) {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(found != cases.end());
        REQUIRE(found->validation.has_value());
        return found->validation->category;
    };
    const auto category = [](std::string_view value) {
        if (value == "invalid_strike") return kiyosi::error_category::invalid_strike;
        if (value == "invalid_schedule") return kiyosi::error_category::invalid_schedule;
        if (value == "invalid_option") return kiyosi::error_category::invalid_option;
        return kiyosi::error_category::invalid_parameter;
    };
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, -1.0, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in).error().category ==
          category(expected("invalid-barrier-strike")));
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in, 10.0,
                                      kiyosi::rebate_timing::at_hit).error().category ==
          category(expected("invalid-barrier-hit-rebate")));
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_out, 10.0, false,
                                             kiyosi::rebate_timing::at_hit).error().category ==
          category(expected("invalid-binary-hit-settlement")));
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_in, 10.0, false,
                                             kiyosi::rebate_timing::at_expiry,
                                             kiyosi::observation_mode::scheduled).error().category ==
          category(expected("invalid-binary-schedule")));
}

TEST_CASE("Asian QuantLib references reconstruct averaging contracts and approximate Greeks")
{
    std::size_t generated = 0;
    std::size_t migrated = 0;
    std::size_t wrapped = 0;
    for (const auto& fixture : kiyosi::test::load_reference_cases(fixture_path())) {
        if (fixture.instrument != "GeometricAverageOption" && fixture.instrument != "ArithmeticAverageOption") continue;
        const auto& inputs = fixture.inputs;
        if (!inputs.contains("owner") && !inputs.contains("reference_provider")) continue;
        INFO("case=" << fixture.case_id);
        const bool owned = inputs.contains("owner");
        REQUIRE(inputs.at(owned ? "owner" : "reference_provider") == "QuantLib");
        REQUIRE(owned == fixture.case_id.starts_with("ql-asian-"));
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        const bool geometric = fixture.instrument == "GeometricAverageOption";
        REQUIRE(inputs.at("averaging") == (geometric ? "geometric" : "arithmetic"));
        REQUIRE(inputs.at("monitoring") == "continuous");
        REQUIRE(inputs.at("settlement") == "expiry");
        REQUIRE(inputs.at("date_roll") == "none");
        REQUIRE(fixture.provenance.convention == "Actual/365 Fixed, continuously compounded BSM");
        REQUIRE((inputs.at("calendar") == "null" || inputs.at("calendar") == "sse"));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        const auto type = inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put;
        REQUIRE(date("effective") <= date("valuation"));
        REQUIRE(date("average_start") <= date("valuation"));
        REQUIRE(date("valuation") <= date("expiry"));
        const bool terminal = date("valuation") == date("expiry");
        REQUIRE(fixture.provenance.source_symbol == (terminal ? "QuantLib.PlainVanillaPayoff" :
            geometric ? "QuantLib.AnalyticContinuousGeometricAveragePriceAsianEngine" : "QuantLib.ContinuousArithmeticAsianLevyEngine"));
        REQUIRE(fixture.provenance.reference_kind == (!geometric && !terminal ? "approximate" : "analytic"));
        if (geometric) {
            REQUIRE(date("average_start") == date("valuation"));
            REQUIRE(number("realized_average") == 0);
        }
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(parameters.has_value());
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"),
            inputs.at("calendar") == "sse" ? kiyosi::sse_calendar() : kiyosi::all_days_calendar());
        REQUIRE(context.has_value());
        const auto check = [&](const auto& option, const auto& engine) {
            REQUIRE(option.has_value());
            const auto native = engine.price(*option, *context);
            check_price(fixture, native);
            for (const auto& [name, measure] : measures)
                REQUIRE(native->get(measure).has_value() == (name == "price"));
            if (!owned) { ++migrated; REQUIRE(fixture.outputs.size() == 1); return; }
            ++generated;
            const bool smooth = !terminal && (date("valuation") - date("average_start")).count() > 2 &&
                (date("expiry") - date("valuation")).count() > 2;
            REQUIRE(inputs.at("wrapper") == (smooth ? "true" : "false"));
            if (!smooth) { REQUIRE(fixture.outputs.size() == 1); return; }
            ++wrapped;
            REQUIRE(fixture.outputs.size() == measures.size());
            const auto numerical = kiyosi::NumericalAnalyticsEngine{engine, kiyosi::NumericalShiftSettings{
                number("spot_shift"), number("volatility_shift"), number("rate_shift"),
                static_cast<int>(number("time_shift_days"))}}.price(*option, *context);
            REQUIRE(numerical.has_value());
            for (const auto& [name, measure] : measures) {
                INFO("measure=" << name);
                REQUIRE(numerical->get(measure).has_value());
                CHECK_THAT(*numerical->get(measure), Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                    number("numerical_tolerance_" + name) + number("uncertainty_" + name)));
            }
        };
        if (geometric) {
            REQUIRE(fixture.engine == "GeometricAverageAsianEngine");
            check(kiyosi::make_geometric_average_option(type, number("strike"), date("average_start"),
                date("effective"), date("expiry"), number("realized_average")), kiyosi::GeometricAverageAsianEngine{});
        } else {
            REQUIRE(fixture.engine == "ArithmeticAverageAsianEngine");
            check(kiyosi::make_arithmetic_average_option(type, number("strike"), date("average_start"),
                date("effective"), date("expiry"), number("realized_average")), kiyosi::ArithmeticAverageAsianEngine{});
        }
    }
    CHECK(generated == 24);
    CHECK(migrated == 4);
    CHECK(wrapped == 6);
}

TEST_CASE("Asian fixtures reject missing boundary declarations and unknown measures")
{
    const auto original = fixture_text();
    for (const auto& identifier : {"ql-asian-geometric-call-100-365d-0elapsed", "ql-asian-arithmetic-call-expiry-100"}) {
        const auto start = original.find(std::string{identifier} + '\t');
        REQUIRE(start != std::string::npos);
        const auto row = original.substr(start, original.find('\n', start) - start);
        const auto declaration = row.find("unavailable_theta=");
        REQUIRE(declaration != std::string::npos);
        auto changed = row;
        changed.erase(declaration, changed.find(';', declaration) - declaration + 1);
        auto text = original;
        text.replace(start, row.size(), changed);
        std::istringstream missing{text};
        CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(missing), kiyosi::test::FixtureParseError);
        changed = row;
        const auto output = changed.find("\tprice=");
        REQUIRE(output != std::string::npos);
        changed.replace(output, 7, "\ttypo=");
        text = original;
        text.replace(start, row.size(), changed);
        std::istringstream unknown{text};
        CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(unknown), kiyosi::test::FixtureParseError);
    }
}

TEST_CASE("Asian engines match pinned terms and market assumptions")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto valuation = effective + std::chrono::days{90};
    const auto expiry = effective + std::chrono::days{180};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto find = [&](std::string_view id) -> const auto& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(found != cases.end());
        return *found;
    };
    const auto arithmetic_call = *kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::call, 100.0, effective, effective, expiry, 101.0);
    const auto arithmetic_put = *kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::put, 100.0, effective, effective, expiry, 101.0);
    check_price(find("asian-arithmetic-call"), kiyosi::ArithmeticAverageAsianEngine{}.price(arithmetic_call, context));
    check_price(find("asian-arithmetic-put"), kiyosi::ArithmeticAverageAsianEngine{}.price(arithmetic_put, context));
    const auto geometric = *kiyosi::make_geometric_average_option(
        kiyosi::option_type::put, 85.0, effective, effective, effective + std::chrono::days{91});
    const auto geometric_context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, -0.03, 0.2), *kiyosi::make_asset_price(80.0), effective);
    check_price(find("asian-geometric"), kiyosi::GeometricAverageAsianEngine{}.price(geometric, geometric_context));
    const auto sse_context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, -0.03, 0.2), *kiyosi::make_asset_price(80.0), effective,
        kiyosi::sse_calendar());
    check_price(find("asian-geometric-sse"), kiyosi::GeometricAverageAsianEngine{}.price(geometric, sse_context));
}

TEST_CASE("Pricing reference public properties cover payoff, in-out, convergence, and seeded paths")
{
    const auto valuation = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 6};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto put = *kiyosi::make_european_put(100.0, expiry);
    const auto analytic_call = *kiyosi::AnalyticEuropeanEngine{}.price(call, context);
    const auto analytic_put = *kiyosi::AnalyticEuropeanEngine{}.price(put, context);
    const auto call_price = *analytic_call.get(kiyosi::risk_measure::price);
    const auto put_price = *analytic_put.get(kiyosi::risk_measure::price);
    CHECK(std::abs(call_price - put_price -
                   (100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04))) < 1e-10);

    const auto in = *kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, expiry, 130.0,
                                                  kiyosi::barrier_type::up_and_in);
    const auto out = *kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, expiry, 130.0,
                                                   kiyosi::barrier_type::up_and_out);
    const auto barrier_in = *kiyosi::AnalyticBarrierEngine{}.price(in, context);
    const auto barrier_out = *kiyosi::AnalyticBarrierEngine{}.price(out, context);
    CHECK(std::abs(*barrier_in.get(kiyosi::risk_measure::price) +
                       *barrier_out.get(kiyosi::risk_measure::price) - call_price) < 2e-5);

    const auto coarse = *kiyosi::BinomialEuropeanEngine{32}.price(call, context);
    const auto fine = *kiyosi::BinomialEuropeanEngine{128}.price(call, context);
    CHECK(std::abs(*fine.get(kiyosi::risk_measure::price) - call_price) <
          std::abs(*coarse.get(kiyosi::risk_measure::price) - call_price));

    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto binomial_fixture = std::ranges::find_if(cases, [](const auto& value) {
        return value.case_id == "european-binomial";
    });
    REQUIRE(binomial_fixture != cases.end());
    REQUIRE(binomial_fixture->convergence.has_value());
    for (const auto resolution : binomial_fixture->convergence->resolutions) {
        const auto result = kiyosi::BinomialEuropeanEngine{static_cast<int>(resolution)}.price(call, context);
        REQUIRE(result.has_value());
        CHECK(std::isfinite(*result->get(kiyosi::risk_measure::price)));
    }
    const auto final_binomial = *kiyosi::BinomialEuropeanEngine{
        static_cast<int>(binomial_fixture->convergence->resolutions.back())}.price(call, context);
    CHECK(std::abs(*final_binomial.get(kiyosi::risk_measure::price) -
                   binomial_fixture->convergence->reference) <= binomial_fixture->convergence->tolerance);

    const auto monte_carlo_fixture = std::ranges::find_if(cases, [](const auto& value) {
        return value.case_id == "european-mc";
    });
    REQUIRE(monte_carlo_fixture != cases.end());
    REQUIRE(monte_carlo_fixture->monte_carlo.has_value());
    const auto& monte_carlo_metadata = *monte_carlo_fixture->monte_carlo;
    const kiyosi::MonteCarloEuropeanEngine monte_carlo{
        static_cast<int>(monte_carlo_metadata.paths), static_cast<int>(monte_carlo_metadata.steps),
        monte_carlo_metadata.seed};
    const auto first = *monte_carlo.price(call, context);
    const auto second = *monte_carlo.price(call, context);
    CHECK(*first.get(kiyosi::risk_measure::price) == *second.get(kiyosi::risk_measure::price));
    CHECK(std::abs(*first.get(kiyosi::risk_measure::price) - monte_carlo_fixture->outputs.at("price")) <=
          monte_carlo_metadata.tolerance);
}

TEST_CASE("Typed structured finite-difference fixtures execute every concrete product")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 1};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), *kiyosi::make_asset_price(100.0), effective);
    const std::vector<kiyosi::date> observations{effective + std::chrono::days{90},
                                                 effective + std::chrono::days{181},
                                                 effective + std::chrono::days{273}, expiry};
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto accumulator = *kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 3.0, effective, expiry);
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.02, 100.0, 75.0, knock_outs, {90.0, 90.0, 90.0, 90.0}, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto snowball = *kiyosi::make_snowball_option(
        coupons, 0.08, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto binary = *kiyosi::make_binary_snowball_option(
        coupons, 0.08, 100.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        coupons, 0.08, 0.02, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto fixture = [&](std::string_view case_id) -> const auto& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        return *found;
    };
    check_price(fixture("accumulator-fd"), kiyosi::FiniteDifferenceAccumulatorEngine{{80, 1024}}.price(accumulator, context));
    check_price(fixture("structured-fd"), kiyosi::FiniteDifferencePhoenixEngine{{80, 1024}}.price(phoenix, context));
    check_price(fixture("snowball-fd"), kiyosi::FiniteDifferenceSnowballEngine{{80, 1024}}.price(snowball, context));
    check_price(fixture("binary-snowball-fd"), kiyosi::FiniteDifferenceBinarySnowballEngine{{80, 1024}}.price(binary, context));
    check_price(fixture("ternary-snowball-fd"), kiyosi::FiniteDifferenceTernarySnowballEngine{{80, 1024}}.price(ternary, context));
}

TEST_CASE("Seeded Monte Carlo fixtures execute every concrete Monte Carlo engine repeatably")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 1};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), *kiyosi::make_asset_price(100.0), effective);
    const auto call = *kiyosi::make_european_call(100.0, effective, expiry);
    const auto put = *kiyosi::make_american_put(100.0, effective, expiry);
    const std::vector<kiyosi::date> observations{effective + std::chrono::days{90},
                                                 effective + std::chrono::days{181},
                                                 effective + std::chrono::days{273}, expiry};
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.02, 100.0, 75.0, knock_outs, {90.0, 90.0, 90.0, 90.0}, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto snowball = *kiyosi::make_snowball_option(
        coupons, 0.08, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto binary = *kiyosi::make_binary_snowball_option(
        coupons, 0.08, 100.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        coupons, 0.08, 0.02, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto require_repeatable = [&](std::string_view case_id, const auto& instrument, const auto& first_engine) {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        REQUIRE(found->monte_carlo.has_value());
        const auto first = first_engine.price(instrument, context);
        const auto second = first_engine.price(instrument, context);
        REQUIRE(first.has_value());
        REQUIRE(second.has_value());
        REQUIRE(first->get(kiyosi::risk_measure::price).has_value());
        REQUIRE(second->get(kiyosi::risk_measure::price).has_value());
        CHECK(*first->get(kiyosi::risk_measure::price) == *second->get(kiyosi::risk_measure::price));
        CHECK(std::isfinite(*first->get(kiyosi::risk_measure::price)));
    };
    require_repeatable("european-mc", call, kiyosi::MonteCarloEuropeanEngine{20000, 252, 42});
    require_repeatable("american-mc", put, kiyosi::MonteCarloAmericanEngine{20000, 50, 42});
    require_repeatable("phoenix-mc", phoenix, kiyosi::MonteCarloPhoenixEngine{{1000, 42}});
    require_repeatable("snowball-mc", snowball, kiyosi::MonteCarloSnowballEngine{{1000, 42}});
    require_repeatable("binary-snowball-mc", binary, kiyosi::MonteCarloBinarySnowballEngine{{1000, 42}});
    require_repeatable("ternary-snowball-mc", ternary, kiyosi::MonteCarloTernarySnowballEngine{{1000, 42}});
}

/*
    const auto cases = kiyosi::test::load_reference_cases(
        std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests/fixtures/structured_reference.tsv");
    REQUIRE_FALSE(cases.empty());
    std::set<std::string> engines;
    for (const auto& fixture : cases) {
        INFO(fixture.case_id);
        const auto& inputs = fixture.inputs;
        const auto number = [&](const std::string& key) { return std::stod(inputs.at(key)); };
        const auto parse_date = [](const std::string& value) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({value}, index, 0, "structured date");
        };
        const auto effective = parse_date(inputs.at("effective"));
        const auto expiry = parse_date(inputs.at("expiry"));
        const auto valuation = parse_date(inputs.at("valuation"));
        const auto calendar = inputs.at("calendar") == "sse" ? kiyosi::sse_calendar() : kiyosi::all_days_calendar();
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(number("spot")), valuation, calendar);
        REQUIRE(context.has_value());
        const auto check = [&](const auto& instrument) {
            REQUIRE(instrument.has_value());
            using Instrument = typename std::remove_cvref_t<decltype(instrument)>::value_type;
            if (fixture.monte_carlo) {
                const auto& budget = *fixture.monte_carlo;
                std::size_t steps = 0;
                for (auto current = valuation + std::chrono::days{1}; current <= expiry; current += std::chrono::days{1})
                    steps += calendar.is_trading_day(current) ? 1 : 0;
                CHECK(budget.steps == steps);
                const kiyosi::MonteCarloStructuredEngine<Instrument> engine{
                    {static_cast<int>(budget.paths), budget.seed}};
                check_price(fixture, engine.price(*instrument, *context));
            } else {
                REQUIRE(inputs.at("scheme") == "crank_nicolson");
                const kiyosi::FiniteDifferenceStructuredEngine<Instrument> engine{{
                    std::stoi(inputs.at("asset_steps")), std::stoi(inputs.at("time_steps")),
                    kiyosi::finite_difference_scheme::crank_nicolson, number("upper_boundary")}};
                check_price(fixture, engine.price(*instrument, *context));
            }
        };
        engines.insert(fixture.engine);
        if (fixture.instrument == "Accumulator") {
            check(kiyosi::make_accumulator(number("strike"), number("knock_out"), number("daily_quantity"),
                number("acceleration"), number("accumulated_quantity"), effective, expiry));
            continue;
        }
        const auto numbers = [&](const std::string& key) {
            std::vector<double> values;
            for (const auto& value : kiyosi::test::detail::split(inputs.at(key), ',')) values.push_back(std::stod(value));
            return values;
        };
        std::vector<kiyosi::date> observations;
        for (const auto& value : kiyosi::test::detail::split(inputs.at("observations"), ',')) observations.push_back(parse_date(value));
        const auto touch = inputs.at("touch") == "down" ? kiyosi::barrier_touch_status::down :
                           inputs.at("touch") == "up" ? kiyosi::barrier_touch_status::up : kiyosi::barrier_touch_status::none;
        const auto frequency = inputs.contains("frequency") && inputs.at("frequency") == "at_expiry" ?
            kiyosi::observation_frequency::at_expiry : kiyosi::observation_frequency::daily;
        if (fixture.instrument == "PhoenixOption")
            check(kiyosi::make_phoenix_option(number("coupon_rate"), number("initial_price"), number("knock_in_price"),
                numbers("knock_out_prices"), numbers("coupon_barriers"), number("upper_strike"), number("lower_strike"),
                observations, frequency, touch, number("principal_ratio"), effective, expiry));
        else if (fixture.instrument == "SnowballOption")
            check(kiyosi::make_snowball_option(numbers("knock_out_coupon_rates"), number("maturity_coupon_rate"),
                number("initial_price"), number("knock_in_price"), numbers("knock_out_prices"), number("upper_strike"),
                number("lower_strike"), observations, frequency, touch, number("principal_ratio"), effective, expiry));
        else if (fixture.instrument == "BinarySnowballOption")
            check(kiyosi::make_binary_snowball_option(numbers("knock_out_coupon_rates"), number("maturity_coupon_rate"),
                number("initial_price"), numbers("knock_out_prices"), number("upper_strike"), number("lower_strike"),
                observations, touch, number("principal_ratio"), effective, expiry));
        else {
            REQUIRE(fixture.instrument == "TernarySnowballOption");
            check(kiyosi::make_ternary_snowball_option(numbers("knock_out_coupon_rates"), number("maturity_coupon_rate"),
                number("minimal_coupon_rate"), number("initial_price"), number("knock_in_price"), numbers("knock_out_prices"),
                number("upper_strike"), number("lower_strike"), observations, frequency, touch, number("principal_ratio"), effective, expiry));
        }
    }
    CHECK(engines.size() == 10);
*/

TEST_CASE("Pricing reference manifest rejects incomplete output tolerances")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tOption\tEngine\tcall\tspot=100;source_revision=08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2;source_symbol=test;convention=Actual/365,BSM;reference_kind=analytic;tolerance=0.1\tprice=1;delta=2\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_reference_cases(input), Catch::Matchers::ContainsSubstring("matching keys"));
}

TEST_CASE("Pricing reference manifest requires complete reference provenance")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tEuropeanOption\tAnalyticEuropeanEngine\tcall\tspot=100\tprice=1\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_reference_cases(input),
                      Catch::Matchers::ContainsSubstring("source_revision"));
}

TEST_CASE("QuantLib continuous barrier portfolios validate prices and numerical Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const std::map<std::string, kiyosi::barrier_type> kinds{
        {"up_and_in", kiyosi::barrier_type::up_and_in}, {"up_and_out", kiyosi::barrier_type::up_and_out},
        {"down_and_in", kiyosi::barrier_type::down_and_in}, {"down_and_out", kiyosi::barrier_type::down_and_out}};
    std::map<std::string, int> generated, wrappers;
    int migrated = 0;
    for (const auto& fixture : cases) {
        const auto& inputs = fixture.inputs;
        if (fixture.instrument != "BarrierOption" ||
            (!inputs.contains("owner") && !inputs.contains("reference_provider"))) continue;
        INFO("case=" << fixture.case_id);
        const bool owned = inputs.contains("owner");
        REQUIRE(inputs.at(owned ? "owner" : "reference_provider") == "QuantLib");
        REQUIRE(fixture.provenance.reference_kind == "analytic");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBarrierEngine+QuantLib.DiscountingBondEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        REQUIRE(kinds.contains(inputs.at("barrier_kind")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE((inputs.at("settlement") == "at_hit" || inputs.at("settlement") == "at_expiry"));
        const auto option = kiyosi::make_barrier_option(
            inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            number("strike"), date("effective"), date("expiry"), number("barrier"), kinds.at(inputs.at("barrier_kind")),
            number("rebate"), inputs.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit : kiyosi::rebate_timing::at_expiry);
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(parameters.has_value());
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"));
        REQUIRE(context.has_value());
        const auto check = [&](const auto& engine) {
            const auto native = engine.price(*option, *context);
            check_price(fixture, native);
            for (const auto& [name, measure] : measures)
                REQUIRE(native->get(measure).has_value() == (name == "price"));
            if (!owned) {
                ++migrated;
                REQUIRE(fixture.outputs.size() == 1);
                if constexpr (std::is_same_v<std::remove_cvref_t<decltype(engine)>, kiyosi::FiniteDifferenceBarrierEngine>) {
                    REQUIRE(fixture.convergence.has_value());
                    const auto& convergence = *fixture.convergence;
                    REQUIRE(convergence.parameter == "asset_steps");
                    REQUIRE(convergence.reference == fixture.outputs.at("price"));
                    for (const auto resolution : convergence.resolutions) {
                        const auto refined = kiyosi::FiniteDifferenceBarrierEngine{{static_cast<int>(resolution),
                            static_cast<int>(number("time_steps")), kiyosi::finite_difference_scheme::crank_nicolson,
                            number("upper_boundary")}}.price(*option, *context);
                        REQUIRE(refined.has_value());
                        if (resolution == convergence.resolutions.back())
                            CHECK_THAT(*refined->get(kiyosi::risk_measure::price),
                                Catch::Matchers::WithinAbs(convergence.reference, convergence.tolerance));
                    }
                }
                return;
            }
            ++generated[fixture.engine];
            REQUIRE((inputs.at("wrapper") == "true" || inputs.at("wrapper") == "false"));
            const bool boundary = (date("expiry") - date("valuation")).count() <= 2;
            if (boundary) REQUIRE(inputs.at("wrapper") == "false");
            for (const auto& [name, value] : fixture.outputs) REQUIRE(measures.contains(name));
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
                CHECK_THAT(*numerical->get(measure), Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                    number("numerical_tolerance_" + name) + number("uncertainty_" + name)));
            }
        };
        if (fixture.engine == "AnalyticBarrierEngine") check(kiyosi::AnalyticBarrierEngine{});
        else {
            REQUIRE(fixture.engine == "FiniteDifferenceBarrierEngine");
            REQUIRE(inputs.at("scheme") == "crank_nicolson");
            check(kiyosi::FiniteDifferenceBarrierEngine{{static_cast<int>(number("asset_steps")),
                static_cast<int>(number("time_steps")), kiyosi::finite_difference_scheme::crank_nicolson, number("upper_boundary")}});
        }
    }
    REQUIRE(migrated == 18);
    REQUIRE(generated.size() == 2);
    CHECK(generated["AnalyticBarrierEngine"] == 60);
    CHECK(generated["FiniteDifferenceBarrierEngine"] == 60);
    CHECK(wrappers["AnalyticBarrierEngine"] == 48);
    CHECK(wrappers["FiniteDifferenceBarrierEngine"] == 12);
}

TEST_CASE("Continuous barrier finite differences enforce the absorbing boundary during each solve")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto fixture = std::ranges::find_if(cases, [](const auto& row) { return row.case_id == "barrier-fd"; });
    REQUIRE(fixture != cases.end());
    const auto option = *kiyosi::make_barrier_option(kiyosi::option_type::call, 100, standard_expiry(),
        95, kiyosi::barrier_type::down_and_in, 10, kiyosi::rebate_timing::at_expiry);
    double previous_error = std::numeric_limits<double>::infinity();
    for (const int resolution : {800, 1600, 3200}) {
        const auto result = kiyosi::FiniteDifferenceBarrierEngine{{resolution, resolution,
            kiyosi::finite_difference_scheme::crank_nicolson, 400}}.price(option, standard_context());
        check_price(*fixture, result);
        const double error = std::abs(*result->get(kiyosi::risk_measure::price) - fixture->outputs.at("price"));
        CAPTURE(resolution, error);
        CHECK(error < previous_error);
        previous_error = error;
    }
}

TEST_CASE("QuantLib binary barrier contracts validate prices and smooth Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const std::map<std::string, kiyosi::barrier_type> kinds{
        {"up_and_in", kiyosi::barrier_type::up_and_in}, {"up_and_out", kiyosi::barrier_type::up_and_out},
        {"down_and_in", kiyosi::barrier_type::down_and_in}, {"down_and_out", kiyosi::barrier_type::down_and_out}};
    int generated = 0, migrated = 0, wrappers = 0;
    for (const auto& fixture : cases) {
        const auto& inputs = fixture.inputs;
        if (fixture.instrument != "BinaryBarrierOption" ||
            (!inputs.contains("owner") && !inputs.contains("reference_provider"))) continue;
        INFO("case=" << fixture.case_id);
        const bool owned = inputs.contains("owner");
        REQUIRE(inputs.at(owned ? "owner" : "reference_provider") == "QuantLib");
        REQUIRE(fixture.engine == "AnalyticBinaryBarrierEngine");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBinaryBarrierEngine+QuantLib.AnalyticDigitalAmericanEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        REQUIRE(kinds.contains(inputs.at("barrier_kind")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put" || inputs.at("option") == "none"));
        REQUIRE((inputs.at("asset_settlement") == "true" || inputs.at("asset_settlement") == "false"));
        REQUIRE((inputs.at("settlement") == "at_hit" || inputs.at("settlement") == "at_expiry"));
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        std::optional<kiyosi::option_type> type;
        if (inputs.at("option") != "none") type = inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put;
        const auto option = kiyosi::make_binary_barrier_option(type, number("strike"), date("effective"), date("expiry"),
            number("barrier"), kinds.at(inputs.at("barrier_kind")), number("payout"), inputs.at("asset_settlement") == "true",
            inputs.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit : kiyosi::rebate_timing::at_expiry);
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(parameters.has_value());
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"));
        REQUIRE(context.has_value());
        const kiyosi::AnalyticBinaryBarrierEngine engine;
        const auto native = engine.price(*option, *context);
        check_price(fixture, native);
        for (const auto& [name, measure] : measures) REQUIRE(native->get(measure).has_value() == (name == "price"));
        for (const auto& [name, value] : fixture.outputs) REQUIRE(measures.contains(name));
        if (!owned) {
            ++migrated;
            REQUIRE(fixture.outputs.size() == 1);
            continue;
        }
        ++generated;
        REQUIRE(fixture.case_id.starts_with("ql-binary-"));
        const bool boundary = number("spot") == number("barrier") || (date("expiry") - date("valuation")).count() <= 2;
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
            REQUIRE(numerical->get(measure).has_value());
            CHECK_THAT(*numerical->get(measure), Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                number("numerical_tolerance_" + name) + number("uncertainty_" + name)));
        }
    }
    CHECK(migrated == 30);
    CHECK(generated == 268);
    CHECK(wrappers == 140);
}

TEST_CASE("Binary boundary declarations reject missing or invented sensitivities")
{
    const auto original = fixture_text();
    for (const auto& [identifier, reason] : std::array{
             std::pair{"ql-binary-cash-none-up-and-in-at-expiry-130-100-365d\t", "spot equals barrier: hit-state boundary"},
             std::pair{"ql-binary-cash-call-up-and-in-at-expiry-100-100-0d\t", "terminal payoff: no smooth sensitivities"}}) {
        const auto start = original.find(identifier);
        REQUIRE(start != std::string::npos);
        const auto end = original.find('\n', start);
        const auto row = original.substr(start, end - start);
        for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
                 {"unavailable_delta=" + std::string{reason} + ";", ""},
                 {"unavailable_delta=" + std::string{reason}, "unavailable_delta=unknown"},
                 {"unit_price=price", "unit_price=price;unavailable_typo=unknown"}}) {
            auto changed = row;
            const auto position = changed.find(from);
            REQUIRE(position != std::string::npos);
            changed.replace(position, from.size(), to);
            auto text = original;
            text.replace(start, row.size(), changed);
            std::istringstream input{text};
            CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(input), kiyosi::test::FixtureParseError);
        }
    }
}
