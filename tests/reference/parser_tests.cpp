#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>

#include <array>
#include <ranges>
#include <set>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#include "support/reference_harness.hpp"

using kiyosi::test::fixture_text;

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

TEST_CASE("Reference fixture parser reports malformed rows")
{
    auto text = fixture_text();
    const auto output = text.find("\tprice=");
    REQUIRE(output != std::string::npos);
    text.replace(output + 1, text.find_first_of(";\t", output + 1) - output - 1, "price=oops");
    auto parse = [&] {
        std::istringstream input{text};
        return kiyosi::test::parse_reference_cases(input);
    };
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("fixture row"));
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("invalid outputs"));

    text = fixture_text();
    const auto tolerance = text.find("\tprice=", text.find('\t', text.find("\tprice=") + 1));
    REQUIRE(tolerance != std::string::npos);
    text.replace(tolerance + 1, text.find_first_of(";\t", tolerance + 1) - tolerance - 1, "price=-1");
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("non-negative"));

    std::size_t index = 0;
    CHECK_THROWS_WITH(kiyosi::test::detail::calendar_date({"2025-0x-06"}, index, 0, "valuation"),
                      Catch::Matchers::ContainsSubstring("valuation"));
}

TEST_CASE("Reference fixture tolerances are inclusive and mismatch reports are useful")
{
    CHECK(kiyosi::test::within_tolerance(2.0, 1.0, 1.0));
    CHECK_FALSE(kiyosi::test::within_tolerance(2.000001, 1.0, 1.0));

    auto fixture = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path()).front();
    const auto actual = fixture.outputs;
    CHECK(kiyosi::test::compare_fixture(fixture, actual).empty());
    fixture.outputs.at("price") += fixture.tolerances.at("price") + 1.0;
    const auto failures = kiyosi::test::compare_fixture(fixture, actual);
    REQUIRE(failures.size() == 1);
    const auto message = failures.front().message();
    CHECK(message.find("case='" + fixture.case_id + "'") != std::string::npos);
    CHECK(message.find("output='price'") != std::string::npos);
    CHECK(message.find("expected=") != std::string::npos);
    CHECK(message.find("actual=") != std::string::npos);
    CHECK(message.find("tolerance=") != std::string::npos);
}

TEST_CASE("Pricing reference manifest covers instruments, engines, and numerical metadata")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    REQUIRE(cases.size() == 692);
    std::vector<std::string> names;
    names.reserve(cases.size());
    for (const auto& value : cases) {
        INFO("case=" << value.case_id << " instrument=" << value.instrument << " engine=" << value.engine);
        REQUIRE(value.case_id.starts_with("ql-"));
        REQUIRE(value.inputs.at("owner") == "QuantLib");
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
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "BarrierOption/FiniteDifferenceBarrierEngine"; }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.monte_carlo.has_value(); }));
}

TEST_CASE("Pricing reference manifest inventories QuantLib supported engines and contracts")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    const std::set<std::string> required_engines{
        "AnalyticBarrierEngine", "AnalyticBinaryBarrierEngine", "AnalyticDigitalEngine",
        "AnalyticEuropeanEngine", "ArithmeticAverageAsianEngine", "BinomialAmericanEngine",
        "BinomialEuropeanEngine", "BjerksundStenslandAmericanEngine", "CrrEngine",
        "FiniteDifferenceAmericanEngine", "FiniteDifferenceBarrierEngine",
        "FiniteDifferenceDigitalEngine", "FiniteDifferenceEuropeanEngine",
        "GeometricAverageAsianEngine", "IntegralDigitalEngine", "IntegralEuropeanEngine",
        "MonteCarloAmericanEngine", "MonteCarloEuropeanEngine"};
    const std::set<std::string> required_instruments{
        "AmericanOption", "ArithmeticAverageOption", "BarrierOption",
        "BinaryBarrierOption", "EuropeanAssetOrNothingOption",
        "EuropeanCashOrNothingOption", "EuropeanOption", "GeometricAverageOption"};
    const std::set<std::string> required_pairs{
        "AmericanOption/BinomialAmericanEngine", "AmericanOption/FiniteDifferenceAmericanEngine",
        "AmericanOption/MonteCarloAmericanEngine", "AmericanOption/BjerksundStenslandAmericanEngine",
        "AmericanOption/CrrEngine",
        "ArithmeticAverageOption/ArithmeticAverageAsianEngine", "BarrierOption/AnalyticBarrierEngine",
        "BarrierOption/FiniteDifferenceBarrierEngine",
        "BinaryBarrierOption/AnalyticBinaryBarrierEngine", "EuropeanAssetOrNothingOption/AnalyticDigitalEngine",
        "EuropeanAssetOrNothingOption/IntegralDigitalEngine", "EuropeanAssetOrNothingOption/FiniteDifferenceDigitalEngine",
        "EuropeanCashOrNothingOption/AnalyticDigitalEngine", "EuropeanCashOrNothingOption/FiniteDifferenceDigitalEngine",
        "EuropeanCashOrNothingOption/IntegralDigitalEngine", "EuropeanOption/AnalyticEuropeanEngine",
        "EuropeanOption/BinomialEuropeanEngine", "EuropeanOption/CrrEngine",
        "EuropeanOption/FiniteDifferenceEuropeanEngine", "EuropeanOption/IntegralEuropeanEngine",
        "EuropeanOption/MonteCarloEuropeanEngine", "GeometricAverageOption/GeometricAverageAsianEngine"};
    std::set<std::string> actual_engines;
    std::set<std::string> actual_instruments;
    std::set<std::string> actual_pairs;
    bool has_settlement = false;
    bool has_monitoring = false;
    bool has_calendar = false;
    for (const auto& value : cases) {
        actual_engines.insert(value.engine);
        actual_instruments.insert(value.instrument);
        actual_pairs.insert(value.instrument + "/" + value.engine);
        has_settlement |= value.inputs.contains("settlement");
        has_monitoring |= value.inputs.contains("monitoring");
        has_calendar |= value.inputs.contains("calendar");
        if (value.engine.find("MonteCarlo") != std::string::npos && !value.validation.has_value())
            REQUIRE(value.monte_carlo.has_value());
        REQUIRE_FALSE(value.validation.has_value());
        REQUIRE_FALSE(value.convergence.has_value());
        REQUIRE(value.outputs.contains("price"));
    }
    CHECK(actual_engines == required_engines);
    CHECK(actual_instruments == required_instruments);
    CHECK(actual_pairs == required_pairs);
    CHECK(has_settlement);
    CHECK(has_monitoring);
    CHECK(has_calendar);
}

TEST_CASE("Pricing reference manifest rejects incomplete output tolerances")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tOption\tEngine\tcall\tspot=100;source_revision=test;source_symbol=test;convention=Actual/365,BSM;reference_kind=analytic;tolerance=0.1\tprice=1;delta=2\tprice=0.1\t-\t-\t-\n"};
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
