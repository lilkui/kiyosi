#pragma once

#include <charconv>
#include <chrono>
#include <concepts>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace kiyosi::test {

using FixtureAttributes = std::map<std::string, std::string>;

class FixtureParseError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

namespace detail {

inline std::vector<std::string> split(std::string_view line, char delimiter)
{
    std::vector<std::string> fields;
    std::size_t start = 0;
    while (start <= line.size()) {
        const auto end = line.find(delimiter, start);
        fields.emplace_back(line.substr(start, end == std::string_view::npos ? end : end - start));
        if (end == std::string_view::npos) break;
        start = end + 1;
    }
    return fields;
}

template <std::integral Integer>
Integer integer(std::string_view text, std::size_t row, std::string_view name)
{
    Integer value{};
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (error != std::errc{} || end != text.data() + text.size())
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + std::string{text} + "'");
    return value;
}

inline Date calendar_date(const std::string& text, std::size_t row, std::string_view name)
{
    if (text.empty())
        throw FixtureParseError("fixture row " + std::to_string(row) + ": empty " + std::string{name});
    if (text.size() == 10 && text[4] == '-' && text[7] == '-') {
        try {
            const std::string_view components{text};
            const auto calendar = std::chrono::year{integer<int>(components.substr(0, 4), row, name)} /
                                  std::chrono::month{integer<unsigned>(components.substr(5, 2), row, name)} /
                                  std::chrono::day{integer<unsigned>(components.substr(8, 2), row, name)};
            if (calendar.ok()) {
                const Date value{calendar};
                if (is_supported_date(value)) return value;
            }
        } catch (const FixtureParseError&) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                    std::string{name} + " '" + text + "' (expected YYYY-MM-DD)");
        }
    }
    throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                            std::string{name} + " '" + text + "' (expected YYYY-MM-DD)");
}

inline FixtureAttributes attributes(std::string_view text, std::size_t row, std::string_view name)
{
    FixtureAttributes result;
    if (text.empty() || text == "-") return result;
    for (const auto& item : split(text, ';')) {
        const auto separator = item.find('=');
        if (separator == std::string::npos || separator == 0 || separator + 1 == item.size()) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                    std::string{name} + " entry '" + item + "'");
        }
        const auto key = item.substr(0, separator);
        if (result.contains(key)) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": duplicate " +
                                    std::string{name} + " key '" + key + "'");
        }
        result.emplace(key, item.substr(separator + 1));
    }
    return result;
}

} // namespace detail

} // namespace kiyosi::test
