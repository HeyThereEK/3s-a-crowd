# Write a function that takes a list of numbers and returns the following string: 
# "air ws air air air ws air air air air"
# where there is a "ws" for every number in the list
# and "air" for every number not in the list
lst = [15, 21, 31, 36, 46, 51, 56, 61, 71, 81, 86, 96, 100]
def signgen(lst):
    return " ".join(["ws" if i in lst else "air" for i in range(0, 128)])
    #return " ".join(["gdc" if i in lst else "gdc" for i in range(0, 128)])



print(signgen(lst)) # "ws air air air ws air air air air"

# Generate the following string: "door level1 2 7 n1 8\ndoor level1 2 7 n2 8\ndoor level1 2 7 n3 8"
# where each n1, n2, n3 is the number in lst
def ginsgen(lst):
    return "\n".join([f"door level1 2 7 {i} 8" for i in lst])

print(ginsgen(lst))