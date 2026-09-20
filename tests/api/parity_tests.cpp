#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <cstdint>
#include <fstream>
#include <kiyosi/kiyosi.hpp>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

namespace {

using Fields = std::unordered_map<std::string, std::string>;

struct ParityCase {
    std::string id;
    std::string kind;
    Fields inputs;
    Fields expected;
    std::string tolerance;
};

Fields fields(std::string text)
{
    Fields result;
    if (text == "-") return result;
    std::istringstream stream{std::move(text)};
    for (std::string item; std::getline(stream, item, ';');) {
        const auto separator = item.find('=');
        REQUIRE(separator != std::string::npos);
        result.emplace(item.substr(0, separator), item.substr(separator + 1));
    }
    return result;
}

std::vector<ParityCase> parity_cases()
{
    std::ifstream stream{KIYOSI_SOURCE_DIR "/tests/fixtures/api_parity.tsv"};
    REQUIRE(stream);
    std::string line;
    std::getline(stream, line);
    std::vector<ParityCase> result;
    while (std::getline(stream, line)) {
        std::istringstream row{line};
        std::vector<std::string> columns;
        for (std::string column; std::getline(row, column, '\t');)
            columns.push_back(column);
        REQUIRE(columns.size() == 5);
        result.push_back({columns[0], columns[1], fields(columns[2]), fields(columns[3]), columns[4]});
    }
    return result;
}

kiyosi::Date parse_date(const std::string& value)
{
    return kiyosi::Date{std::chrono::year{std::stoi(value.substr(0, 4))} /
                        std::chrono::month{static_cast<unsigned>(std::stoul(value.substr(5, 2)))} /
                        std::chrono::day{static_cast<unsigned>(std::stoul(value.substr(8, 2)))}};
}

kiyosi::Timestamp parse_timestamp(const std::string& value)
{
    return kiyosi::start_of_day(parse_date(value)) +
           std::chrono::hours{std::stoi(value.substr(11, 2))} +
           std::chrono::minutes{std::stoi(value.substr(14, 2))} +
           std::chrono::seconds{std::stoi(value.substr(17, 2))} +
           std::chrono::microseconds{std::stoi(value.substr(20, 6))};
}

kiyosi::OptionType option_type(const Fields& values)
{
    REQUIRE(values.at("type") == "call");
    return kiyosi::OptionType::call;
}

kiyosi::BlackScholesMertonParameters parameters(const Fields& values)
{
    const auto result = kiyosi::make_bsm_parameters(
        std::stod(values.at("risk_free_rate")), std::stod(values.at("dividend_yield")),
        std::stod(values.at("volatility")));
    REQUIRE(result);
    return *result;
}

} // namespace

TEST_CASE("C++ public API matches the shared language parity cases", "[api][parity]")
{
    const auto cases = parity_cases();
    REQUIRE(cases.size() == 7);

    for (const auto& test : cases) {
        DYNAMIC_SECTION(test.id)
        {
            if (test.kind == "construction") {
                const auto option = kiyosi::make_european_option(
                    option_type(test.inputs), std::stod(test.inputs.at("strike")),
                    parse_date(test.inputs.at("effective_date")), parse_date(test.inputs.at("expiry_date")));
                REQUIRE(option);
                CHECK(option->option_type() == option_type(test.expected));
                CHECK(option->strike() == std::stod(test.expected.at("strike")));
                CHECK(option->effective_date() == parse_date(test.expected.at("effective_date")));
                CHECK(option->expiry_date() == parse_date(test.expected.at("expiry_date")));
            } else if (test.kind == "defaults") {
                const auto settings = kiyosi::FiniteDifferenceVanillaEngine{}.settings();
                CHECK(settings.asset_step_count == std::stoi(test.expected.at("asset_step_count")));
                CHECK(settings.time_step_count == std::stoi(test.expected.at("time_step_count")));
                REQUIRE(test.expected.at("scheme") == "crank_nicolson");
                CHECK(settings.scheme == kiyosi::FiniteDifferenceScheme::crank_nicolson);
                REQUIRE(test.expected.at("asset_upper_boundary") == "none");
                CHECK_FALSE(settings.asset_upper_boundary);
            } else if (test.kind == "pricing") {
                const auto option = kiyosi::make_european_option(
                    option_type(test.inputs), std::stod(test.inputs.at("strike")),
                    parse_date(test.inputs.at("effective_date")), parse_date(test.inputs.at("expiry_date")));
                const auto context = kiyosi::make_pricing_context(
                    parameters(test.inputs), std::stod(test.inputs.at("spot_price")),
                    parse_date(test.inputs.at("valuation_date")));
                REQUIRE(option);
                REQUIRE(context);
                const auto result = kiyosi::AnalyticVanillaEngine{}.price(*option, *context);
                REQUIRE(result);
                REQUIRE(result->require(kiyosi::RiskMeasure::price));
                CHECK_THAT(*result->require(kiyosi::RiskMeasure::price), Catch::Matchers::WithinAbs(
                                                                             std::stod(test.expected.at("price")), std::stod(test.tolerance)));
            } else if (test.kind == "domain_error") {
                const auto result = kiyosi::make_bsm_parameters(
                    std::stod(test.inputs.at("risk_free_rate")),
                    std::stod(test.inputs.at("dividend_yield")),
                    std::stod(test.inputs.at("volatility")));
                REQUIRE(test.expected.at("category") == "invalid_volatility");
                REQUIRE_FALSE(result);
                CHECK(result.error().category == kiyosi::ErrorCategory::invalid_volatility);
            } else if (test.kind == "date_round_trip") {
                const auto option = kiyosi::make_geometric_average_option(
                    option_type(test.inputs), std::stod(test.inputs.at("strike")),
                    parse_date(test.inputs.at("averaging_start_date")),
                    parse_date(test.inputs.at("effective_date")), parse_date(test.inputs.at("expiry_date")));
                REQUIRE(option);
                CHECK(option->averaging_start_date() == parse_date(test.expected.at("averaging_start_date")));
                CHECK(option->effective_date() == parse_date(test.expected.at("effective_date")));
                CHECK(option->expiry_date() == parse_date(test.expected.at("expiry_date")));
            } else if (test.kind == "timestamp_round_trip") {
                const auto context = kiyosi::make_pricing_context(
                    parameters(test.inputs), std::stod(test.inputs.at("spot_price")),
                    parse_timestamp(test.inputs.at("valuation_time")));
                REQUIRE(context);
                CHECK(context->valuation_time() == parse_timestamp(test.expected.at("valuation_time")));
                CHECK(context->valuation_date() == parse_date(test.expected.at("valuation_date")));
            } else if (test.kind == "numeric_boundary") {
                const auto seed = std::stoull(test.inputs.at("seed"));
                const kiyosi::MonteCarloVanillaEngine engine{
                    kiyosi::MonteCarloSettings{100'000, 50, seed}};
                REQUIRE(engine.settings().seed);
                CHECK(*engine.settings().seed == std::stoull(test.expected.at("seed")));
            } else {
                FAIL("unknown parity case kind: " << test.kind);
            }
        }
    }
}
