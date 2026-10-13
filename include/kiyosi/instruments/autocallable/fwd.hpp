#pragma once

#include <concepts>

namespace kiyosi {

struct PhoenixTerms;
struct SnowballTerms;
struct BinarySnowballTerms;
struct TernarySnowballTerms;

template <typename Terms>
concept AutocallableTerms = std::same_as<Terms, PhoenixTerms> || std::same_as<Terms, SnowballTerms> ||
                            std::same_as<Terms, BinarySnowballTerms> || std::same_as<Terms, TernarySnowballTerms>;

template <AutocallableTerms Terms>
class AutocallableNote;

/// Knock-in note paying conditional coupons at each observation.
using PhoenixOption = AutocallableNote<PhoenixTerms>;
/// Knock-in note with downside participation and knock-out coupons.
using SnowballOption = AutocallableNote<SnowballTerms>;
/// Note settling a flat maturity coupon regardless of terminal spot.
using BinarySnowballOption = AutocallableNote<BinarySnowballTerms>;
/// Knock-in note whose maturity coupon steps down to a floor.
using TernarySnowballOption = AutocallableNote<TernarySnowballTerms>;

/// Complete products supported by the autocallable pricing engines.
template <typename Note>
concept AutocallableProduct = std::same_as<Note, PhoenixOption> ||
                              std::same_as<Note, SnowballOption> ||
                              std::same_as<Note, BinarySnowballOption> ||
                              std::same_as<Note, TernarySnowballOption>;

} // namespace kiyosi
