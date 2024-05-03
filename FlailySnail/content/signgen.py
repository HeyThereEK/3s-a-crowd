
import random

obstacles = {}

for i in range(0, 15):
    start = set[-1] + 7 if len(set) > 0 else 15
    obstacles.add(random.randint(start, 127))
# Generates a list of obstacles like this: 
# [15, 21, 31, 36, 46, 51, 56, 61, 71, 81, 86, 96, 100]

def signgen(obstacles):
    return " ".join(["ws" if i in obstacles else "air" for i in range(0, 128)])

print(signgen(obstacles)) 

def ginsgen(obstacles):
    return "\n".join([f"door level1 2 7 {i} 8" for i in obstacles])

print(ginsgen(obstacles))
