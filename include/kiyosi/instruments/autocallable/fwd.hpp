#pragma once

#include <concepts>

namespace kiyosi {

class PhoenixOption;
class SnowballOption;
class BinarySnowballOption;
class TernarySnowballOption;

/// Complete products supported by the autocallable pricing engines.
template <typename Note>
concept AutocallableProduct = std::same_as<Note, PhoenixOption> ||
                              std::same_as<Note, SnowballOption> ||
                              std::same_as<Note, BinarySnowballOption> ||
                              std::same_as<Note, TernarySnowballOption>;

} // namespace kiyosi
