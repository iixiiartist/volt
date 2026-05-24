def is_anagram(s, t):
    return sorted(s) == sorted(t)

print(is_anagram('anagram', 'nagaram'))
print(is_anagram('rat', 'car'))