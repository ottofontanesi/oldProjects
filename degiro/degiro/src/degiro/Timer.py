import time
import numpy as np

class Timer:
    def __init__(self, total_iterations):
        self.start_time = None # start_time
        self.total_iterations = total_iterations
        self.stop_time = None
        self.iteration = None
        self.exec_time_by_iteration = []
        self.residual_iterations = None

    def time_parser(self, start=None, end=None):
        if start == None:
            start = self.start_time
        if end == None:
            end = self.stop_time
        hours, rem = divmod(end - start, 3600)
        minutes, seconds = divmod(rem, 60)
        return int(hours), int(minutes), int(seconds)

    def timer_from_base_sec(self, rem):
        minutes, seconds = divmod(rem, 60)
        return int(minutes), int(seconds)

    def update_timer(self, start_time, iteration):
        self.iteration = iteration
        self.start_time = start_time
        self.stop_time = time.time()
        time_base_sec = self.stop_time - self.start_time
        self.exec_time_by_iteration.append(time_base_sec)
        self.residual_iterations = self.total_iterations - self.iteration

    def get_time_stats(self):
        hours, minutes, seconds = self.time_parser(start=self.start_time, end=self.stop_time)
        print("--- {:0>2}:{:0>2}:{:0>2} ---> Execution time (min) : last iteration".format(int(hours),int(minutes),seconds))

    def get_avg_time_stats(self):
        avg_exc_time_hours, avg_exc_time_minutes, avg_exc_time_seconds = self.time_parser(start=0, end=np.mean(self.exec_time_by_iteration))
        print("--- {:0>2}:{:0>2}:{:0>2} ---> Average execution time (min) for {} iterations".format(avg_exc_time_hours,avg_exc_time_minutes, avg_exc_time_seconds, self.iteration))

    def get_residual_time_stats(self):
        exp_res_hours, exp_res_minutes, exp_res_seconds = self.time_parser(end=self.residual_iterations*np.mean(self.exec_time_by_iteration), start=0)
        print("--- {:0>2}:{:0>2}:{:0>2} ---> Remaining estimated time (min) for {} iterations".format(exp_res_hours, exp_res_minutes, exp_res_seconds, self.residual_iterations))

    def get_past_time_stats(self):
        past_hours, past_minutes, past_seconds = self.time_parser(end=sum(self.exec_time_by_iteration), start=0)
        print("--- {:0>2}:{:0>2}:{:0>2} ---> Total time (min) spent on {} iterations".format(past_hours, past_minutes, past_seconds, len(self.exec_time_by_iteration)))

    def get_stats(self, start_time, iteration):
        self.update_timer(start_time, iteration)
        self.get_time_stats()
        self.get_avg_time_stats()
        self.get_residual_time_stats()
        self.get_past_time_stats()