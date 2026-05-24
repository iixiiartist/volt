def longest_common_prefix(strs):
    if not strs:
        return ""
    prefix = min(strs, key=len)
    for i, char in enumerate(prefix):
        for other in strs:
            if other[i] != char:
                return prefix[:i]
    return prefix

print(longest_common_prefix(['flower','flow','flight']))
print(longest_common_prefix(['dog','racecar','car']))
print(longest_common_prefix(['']))